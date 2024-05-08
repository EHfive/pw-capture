use crate::*;

use anyhow::Context;

use std::rc::Rc;
use std::thread;
use std::{cell::RefCell, fmt::Debug};

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, Sender};
use dashmap::DashMap;
use educe::Educe;
use log::{debug, trace};
use pipewire as pw;
use pw::main_loop::MainLoop as PwMainLoop;
use pw::properties::properties;
use self_cell::self_cell;
use trait_enumizer::{crossbeam_class, enumizer};

#[enumizer(
    name=ClientMessage,
    pub,
    returnval=crossbeam_class,
    call_fn(name=try_call_mut,ref_mut),
    proxy(Fn, name=ClientMethodsProxy),
    enum_attr[derive(Debug)],
)]
pub trait ClientMethods {
    fn terminate(&self);
    fn create_stream(&mut self, info: StreamInfo) -> Result<Stream>;
}

#[derive(Clone)]
struct ClientImpl {
    inner: Rc<RefCell<ClientImplInner>>,
}

struct ClientImplInner {
    mainloop: pw::main_loop::MainLoop,
    core: pw::core::Core,
    stream_next_id: usize,
    stream_map: DashMap<usize, (StreamImpl, OwnedReceiver)>,
}

type StreamMessageReceiver<'a> = pw::channel::AttachedReceiver<'a, StreamMessage>;

self_cell!(
    struct OwnedReceiver {
        owner: PwMainLoop,

        #[covariant]
        dependent: StreamMessageReceiver,
    }
);

impl ClientMethods for ClientImpl {
    fn terminate(&self) {
        let inner = self.inner.borrow();
        inner.stream_map.clear();
        inner.mainloop.quit();
    }

    fn create_stream(&mut self, info: StreamInfo) -> Result<Stream> {
        debug!("create stream");

        let id = self.inner.borrow().stream_next_id;
        self.inner.borrow_mut().stream_next_id += 1;

        let stream_impl = StreamImpl::new(&self.inner.borrow().core, info, {
            let inner_weak = Rc::downgrade(&self.inner);
            Box::new(move || {
                if let Some(inner) = inner_weak.upgrade() {
                    inner
                        .borrow()
                        .stream_map
                        .remove(&id)
                        .expect("stream_impl already removed");
                }
            })
        })?;

        let mainloop = self.inner.borrow().mainloop.clone();
        let (pw_sender, pw_receiver) = pw::channel::channel::<StreamMessage>();
        let receiver = OwnedReceiver::new(mainloop, |mainloop| {
            stream_impl.attach(mainloop.loop_(), pw_receiver)
        });

        self.inner
            .borrow_mut()
            .stream_map
            .insert(id, (stream_impl, receiver));

        Ok(Stream { pw_sender })
    }
}

#[derive(Educe)]
#[educe(Debug)]
pub struct Stream {
    #[educe(Debug(ignore))]
    pub(crate) pw_sender: pipewire::channel::Sender<StreamMessage>,
}

impl Stream {
    pub fn proxy(
        &self,
    ) -> StreamMethodsProxy<anyhow::Error, impl Fn(StreamMessage) -> Result<(), anyhow::Error>>
    {
        let pw_sender = self.pw_sender.clone();
        StreamMethodsProxy(move |msg| {
            pw_sender
                .send(msg)
                .map_err(|e| anyhow!("failed to send {e:?}"))
        })
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.proxy().try_terminate();
    }
}

#[derive(Educe)]
#[educe(Debug)]
pub struct Client {
    #[educe(Debug(ignore))]
    pw_sender: pw::channel::Sender<ClientMessage>,
    pw_thread: Option<thread::JoinHandle<Result<()>>>,
}

impl Client {
    pub fn new() -> Result<Self> {
        debug!("creating client");
        let (done_sender, done_receiver) = bounded(1);
        let (pw_sender, pw_receiver) = pw::channel::channel::<ClientMessage>();
        let pw_thread = thread::spawn(move || pw_thread(done_sender, pw_receiver));

        done_receiver
            .recv()
            .context("failed to connect PipeWire Server")?;

        Ok(Self {
            pw_sender,
            pw_thread: Some(pw_thread),
        })
    }

    pub fn proxy(
        &self,
    ) -> ClientMethodsProxy<anyhow::Error, impl Fn(ClientMessage) -> Result<(), anyhow::Error>>
    {
        let pw_sender = self.pw_sender.clone();
        ClientMethodsProxy(move |msg| {
            pw_sender
                .send(msg)
                .map_err(|e| anyhow!("failed to send {e:?}"))
        })
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let proxy = self.proxy();
        if proxy.try_terminate().is_err() {
            return;
        }
        if let Some(th) = self.pw_thread.take() {
            let _ = th.join();
        }
    }
}

mod pw_guard {
    pub(super) struct PipeWireGuard(());

    impl PipeWireGuard {
        pub(super) fn new() -> Self {
            pipewire::init();
            Self(())
        }
    }

    impl Drop for PipeWireGuard {
        fn drop(&mut self) {
            unsafe { pipewire::deinit() };
        }
    }
}

fn pw_thread(
    done_sender: Sender<()>,
    pw_receiver: pw::channel::Receiver<ClientMessage>,
) -> Result<()> {
    let _ = pw_guard::PipeWireGuard::new();

    let mainloop = pw::main_loop::MainLoop::new(None)?;

    let context = pw::context::Context::with_properties(
        &mainloop,
        properties! {
            *pw::keys::CONFIG_NAME => "client-rt.conf",
            *pw::keys::APP_NAME => get_app_name(),
        },
    )?;

    let core = context.connect(None)?;

    debug!("{:?}", core);

    let client_impl_inner = ClientImplInner {
        mainloop: mainloop.clone(),
        core,
        stream_next_id: 0,
        stream_map: DashMap::new(),
    };
    let client_impl = RefCell::new(ClientImpl {
        inner: Rc::new(RefCell::new(client_impl_inner)),
    });
    let _receiver = pw_receiver.attach(mainloop.loop_(), {
        move |msg| {
            trace!("receive {:?}", msg);
            let _ = msg.try_call_mut(&mut *client_impl.borrow_mut());
        }
    });

    done_sender.send(())?;
    drop(done_sender);

    mainloop.run();

    Ok(())
}
