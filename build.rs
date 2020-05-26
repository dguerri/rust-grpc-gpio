fn main() {
    tonic_build::compile_protos("proto/remote_gpio_service.proto").unwrap();
}
