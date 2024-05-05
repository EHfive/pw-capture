use core::ffi::CStr;
use core::mem;
use core::ops::{ControlFlow, Range};
use core::slice;

use elf::endian::NativeEndian;
use libc::{c_char, c_int, c_void, dl_phdr_info, size_t};

struct DlIteratePhdrData<'a> {
    f: Box<dyn FnMut(&dl_phdr_info) -> ControlFlow<()> + 'a>,
}

unsafe extern "C" fn dl_iterate_phdr_callback(
    info: *mut dl_phdr_info,
    _size: size_t,
    data: *mut c_void,
) -> c_int {
    debug_assert!(!info.is_null());
    debug_assert!(!data.is_null());
    let data = &mut *(data as *mut DlIteratePhdrData);
    match (data.f)(&mut *info) {
        ControlFlow::Continue(_) => 0,
        ControlFlow::Break(_) => -1,
    }
}

pub fn dl_iterate_phdr<F>(f: F)
where
    F: FnMut(&dl_phdr_info) -> ControlFlow<()>,
{
    let mut data = DlIteratePhdrData { f: Box::new(f) };
    unsafe {
        libc::dl_iterate_phdr(
            Some(dl_iterate_phdr_callback),
            mem::transmute(&mut data as *mut _),
        );
    }
}

#[cfg(target_pointer_width = "64")]
pub const ELF_CLASS: elf::file::Class = elf::file::Class::ELF64;
#[cfg(target_pointer_width = "32")]
pub const ELF_CLASS: elf::file::Class = elf::file::Class::ELF32;

#[cfg(target_pointer_width = "64")]
pub type ElfAddress = u64;
#[cfg(target_pointer_width = "32")]
pub type ElfAddress = u32;

pub struct ObjectInfo {
    relocation: ElfAddress,
    gnu_hash_table: Option<elf::hash::GnuHashTable<'static, NativeEndian>>,
    sysv_hash_table: Option<elf::hash::SysVHashTable<'static, NativeEndian>>,
    string_table: elf::string_table::StringTable<'static>,
    symbol_table: elf::symbol::SymbolTable<'static, NativeEndian>,
}

impl ObjectInfo {
    pub unsafe fn new(info: &dl_phdr_info) -> Option<Self> {
        parse_dl_phdr_info(info)
    }

    pub fn find_symbol_addr(&self, name: &CStr) -> Option<ElfAddress> {
        let (_idx, symbol) = if let Some(table) = &self.gnu_hash_table {
            table
                .find(name.to_bytes(), &self.symbol_table, &self.string_table)
                .ok()??
        } else if let Some(table) = &self.sysv_hash_table {
            table
                .find(name.to_bytes(), &self.symbol_table, &self.string_table)
                .ok()??
        } else {
            return None;
        };
        let addr = self.relocation + symbol.st_value as ElfAddress;
        if addr == 0 {
            return None;
        }
        Some(addr)
    }
}

unsafe fn parse_dl_phdr_info(info: &dl_phdr_info) -> Option<ObjectInfo> {
    let relocation = info.dlpi_addr;
    let phdrs = slice::from_raw_parts(info.dlpi_phdr, info.dlpi_phnum as _);
    let mut mem_range: Range<ElfAddress> = Default::default();
    for phdr in phdrs {
        if phdr.p_type == libc::PT_LOAD {
            let start = relocation + phdr.p_paddr;
            let end = start + phdr.p_memsz;
            mem_range = start..end;
            break;
        }
    }
    if mem_range.is_empty() {
        return None;
    }

    let mut dyn_table: Option<elf::dynamic::DynamicTable<'static, elf::endian::NativeEndian>> =
        None;

    for phdr in phdrs {
        if phdr.p_type != libc::PT_DYNAMIC {
            continue;
        }
        let addr = (relocation + phdr.p_paddr) as *const u8;
        let data = slice::from_raw_parts(addr, phdr.p_memsz as usize);

        dyn_table = Some(elf::dynamic::DynamicTable::new(
            NativeEndian,
            ELF_CLASS,
            data,
        ));
        break;
    }
    let dyn_table = dyn_table?;

    let mut gnu_hash_table: Option<elf::hash::GnuHashTable<'static, NativeEndian>> = None;
    let mut sysv_hash_table: Option<elf::hash::SysVHashTable<'static, NativeEndian>> = None;
    let mut string_table: Option<elf::string_table::StringTable<'static>> = None;
    let mut symbol_table: Option<elf::symbol::SymbolTable<'static, NativeEndian>> = None;

    for elf_dyn in dyn_table {
        let d_tag = elf_dyn.d_tag;
        let start = elf_dyn.d_ptr() as ElfAddress;
        if !mem_range.contains(&start) {
            continue;
        };
        let data = slice::from_raw_parts(start as *const u8, (mem_range.end - start) as usize);

        match d_tag {
            elf::abi::DT_GNU_HASH => {
                gnu_hash_table = elf::hash::GnuHashTable::new(NativeEndian, ELF_CLASS, data).ok();
            }
            elf::abi::DT_HASH => {
                sysv_hash_table = elf::hash::SysVHashTable::new(NativeEndian, ELF_CLASS, data).ok();
            }
            elf::abi::DT_STRTAB => string_table = Some(elf::string_table::StringTable::new(data)),
            elf::abi::DT_SYMTAB => {
                symbol_table = Some(elf::symbol::SymbolTable::new(NativeEndian, ELF_CLASS, data))
            }
            _ => (),
        }
    }

    if gnu_hash_table.is_none() && sysv_hash_table.is_none() {
        return None;
    }

    Some(ObjectInfo {
        relocation,
        gnu_hash_table,
        sysv_hash_table,
        string_table: string_table?,
        symbol_table: symbol_table?,
    })
}

pub type DlsymFunc =
    unsafe extern "C" fn(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
pub type DlvsymFunc = unsafe extern "C" fn(
    handle: *mut c_void,
    symbol: *const c_char,
    version: *const c_char,
) -> *mut c_void;

/// Loads real dlsym and dlvsym backed by libdl or libc
pub fn load_dlsym_dlvsym() -> (Option<DlsymFunc>, Option<DlvsymFunc>) {
    let mut dlsym: Option<DlsymFunc> = None;
    let mut dlvsym: Option<DlvsymFunc> = None;
    dl_iterate_phdr(|info| unsafe {
        let name = CStr::from_ptr(info.dlpi_name);
        let name = name.to_string_lossy();
        if !(name.contains("libdl.so") || name.contains("libc.so")) {
            return ControlFlow::Continue(());
        }
        if let Some(obj) = ObjectInfo::new(info) {
            let name = CStr::from_bytes_with_nul_unchecked(b"dlsym\0");
            dlsym = obj.find_symbol_addr(name).map(|p| mem::transmute(p));
            let name = CStr::from_bytes_with_nul_unchecked(b"dlvsym\0");
            dlvsym = obj.find_symbol_addr(name).map(|p| mem::transmute(p));
        }

        if dlsym.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    (dlsym, dlvsym)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterate_phdr() {
        let mut count = 0;
        dl_iterate_phdr(|info| unsafe {
            let _ = ObjectInfo::new(info);
            count += 1;
            ControlFlow::Break(())
        });
        assert_eq!(count, 1);
    }

    #[test]
    fn load_dlsym() {
        let (dlsym, dlvsym) = load_dlsym_dlvsym();
        unsafe {
            if let Some(dlsym) = dlsym {
                let name = CStr::from_bytes_with_nul_unchecked(b"dlsym\0");
                let dlsym1 = dlsym(libc::RTLD_DEFAULT, name.as_ptr());
                assert!(!dlsym1.is_null());
                let _dlsym1: DlsymFunc = mem::transmute(dlsym1);
            }

            if let Some(dlvsym) = dlvsym {
                let name = CStr::from_bytes_with_nul_unchecked(b"dlvsym\0");
                let version = CStr::from_bytes_with_nul_unchecked(b"GLIBC_2.2.5\0");
                let _dlvsym1: DlvsymFunc =
                    mem::transmute(dlvsym(libc::RTLD_DEFAULT, name.as_ptr(), version.as_ptr()));
            }
        }
    }

    #[test]
    #[ignore = "verbose"]
    fn iterate_phdr_print() {
        dl_iterate_phdr(|info| unsafe {
            let name = CStr::from_ptr(info.dlpi_name);
            let phdrs = slice::from_raw_parts(info.dlpi_phdr, info.dlpi_phnum as _);
            println!("--- {} ---", name.to_string_lossy());
            for phdr in phdrs {
                let addr = (info.dlpi_addr + phdr.p_vaddr) as *const ();
                println!(
                    "  addr: {:?}, type: {}, mem size: {}",
                    addr, phdr.p_type, phdr.p_memsz
                );
            }
            if let Some(obj) = ObjectInfo::new(info) {
                let name = CStr::from_bytes_with_nul_unchecked(b"dlsym\0");
                let symbol = obj.find_symbol_addr(name);
                if let Some(symbol) = symbol {
                    let dlsym: DlsymFunc = mem::transmute(symbol);
                    let dlsym1: DlsymFunc = mem::transmute(dlsym(libc::RTLD_NEXT, name.as_ptr()));
                    println!("dlsym: {:?} {:?}", dlsym, dlsym1);
                    assert_eq!(dlsym, dlsym1);
                }
                let name = CStr::from_bytes_with_nul_unchecked(b"dlvsym\0");
                let symbol = obj.find_symbol_addr(name);
                if let Some(symbol) = symbol {
                    let dlvsym: DlvsymFunc = mem::transmute(symbol);
                    let version = CStr::from_bytes_with_nul_unchecked(b"GLIBC_2.2.5\0");
                    let dlvsym1: DlvsymFunc =
                        mem::transmute(dlvsym(libc::RTLD_DEFAULT, name.as_ptr(), version.as_ptr()));
                    println!(
                        "dlvsym: {:?} {:?}({})",
                        dlvsym,
                        dlvsym1,
                        version.to_string_lossy(),
                    );
                }
            }
            println!();

            ControlFlow::Continue(())
        });
    }
}
