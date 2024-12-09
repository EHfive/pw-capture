project(
  'pw-capture', 'rust',
  version: '0.0.1',
  license: 'MIT',
)

dependency('libpipewire-0.3', version: '>=0.3.41', required: true)
dependency('libspa-0.2', required: true)

cargo = find_program('cargo', required: true)
meson_cargo = find_program('build-aux/meson-cargo.sh')

prefix = get_option('prefix')
bindir = get_option('bindir')
libdir = get_option('libdir')
datadir = get_option('datadir')
profile = get_option('profile')
rust_target = get_option('target')
frozen = 'false'
if get_option('frozen')
  frozen = 'true'
endif

lib_pw_capture_gl_name = 'libpw-capture-gl.so'
lib_pw_capture_gl = custom_target('pw-capture-gl',
  output: lib_pw_capture_gl_name,
  command: [
    meson_cargo,
    '@SOURCE_ROOT@',
    '@BUILD_ROOT@',
    profile,
    'pw-capture-gl',
    'libpw_capture_gl.so',
    '@OUTPUT@',
    frozen,
    rust_target
  ],
  console: true,
  build_always_stale: true,
  install: true,
  install_dir: libdir
)


lib_pw_capture_vk_name = 'libVkLayer_EH5_pwcapture.so'
lib_pw_capture_vk = custom_target('pw-capture-vk',
  output: lib_pw_capture_vk_name,
  command: [
    meson_cargo,
    '@SOURCE_ROOT@',
    '@BUILD_ROOT@',
    profile,
    'pw-capture-vk',
    'libpw_capture_vk.so',
    '@OUTPUT@',
    frozen,
    rust_target
  ],
  console: true,
  build_always_stale: true,
  install: true,
  install_dir: libdir
)


vk_config = configuration_data()
vk_config.set_quoted('LIBRARY', join_paths(prefix, '$LIB', lib_pw_capture_vk_name))
vk_layer_manifest = configure_file(
  input: 'build-aux/layer-manifest.json.in',
  output: 'VkLayer_EH5_pwcapture.json',
  configuration: vk_config,
  install: true,
  install_dir: join_paths(datadir, 'vulkan/implicit_layer.d')
)

script_config = configuration_data()
script_config.set('GL_LIB_NAME', lib_pw_capture_gl_name)
script_config.set('GL_LIB_DIR', prefix + '/\$LIB')
pw_capture_script = configure_file(
  input: 'build-aux/pw-capture.sh.in',
  output: 'pw-capture.sh',
  configuration: script_config,
)

install = find_program('install')
pw_capture = custom_target('pw-capture',
  input: pw_capture_script,
  output: 'pw-capture',
  command: [
    install,
    '-m0755',
    '-T',
    '@INPUT@',
    '@OUTPUT@'
  ],
  install: true,
  install_dir: bindir
)
