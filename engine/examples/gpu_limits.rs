//! Prints the GPU adapter's buffer limits (what the compositor's pool may use).
fn main() {
    let instance = wgpu::Instance::default();
    let a = pollster::block_on(instance.request_adapter(&Default::default())).expect("no adapter");
    let l = a.limits();
    println!("{}: max_storage_buffer_binding_size {} MB, max_buffer_size {} MB", a.get_info().name, l.max_storage_buffer_binding_size >> 20, l.max_buffer_size >> 20);
}
