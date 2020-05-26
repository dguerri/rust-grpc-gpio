use std::collections::HashMap;
use std::sync::Arc;

use env_logger::{self, Env};
use futures::StreamExt;
use gpio_cdev::{Chip, LineHandle, LineRequestFlags};
use log;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, MutexGuard};
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use remote_gpio_service::remote_gpio_service_server::{RemoteGpioService, RemoteGpioServiceServer};
use remote_gpio_service::{
    gpio_write_request, GpioReadRequest, GpioReadResponse, GpioWriteRequest, GpioWriteResponse,
};

pub mod remote_gpio_service {
    tonic::include_proto!("remote_gpio_service");
}

#[derive(Debug)]
pub struct RemoteGpio {
    gpio_chip: Arc<Mutex<Chip>>,
    gpio_lines: Arc<Mutex<HashMap<u32, LineHandle>>>,
}

impl RemoteGpio {
    fn new(chip_name: &str) -> RemoteGpio {
        RemoteGpio {
            gpio_chip: Arc::new(Mutex::new(
                Chip::new(format!("/dev/{}", chip_name)).expect("failed to locate the gpio chip"),
            )),
            gpio_lines: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn get_new_line_handle(mut gpio_chip: MutexGuard<Chip>, pin: u32) -> LineHandle {
    gpio_chip
        .get_line(pin)
        .expect("failed to get gpio line")
        .request(
            LineRequestFlags::INPUT | LineRequestFlags::OUTPUT,
            0,
            "RemoteGpio",
        )
        .expect("failed to operate the gpio pin")
}

#[tonic::async_trait]
impl RemoteGpioService for RemoteGpio {
    type ReadGpioStream = mpsc::Receiver<Result<GpioReadResponse, Status>>;

    async fn read_gpio(
        &self,
        request: Request<GpioReadRequest>,
    ) -> Result<Response<Self::ReadGpioStream>, Status> {
        let request = request.into_inner();

        let (mut tx, rx) = mpsc::channel(4);

        let gpio_chip = self.gpio_chip.clone();
        let gpio_lines = self.gpio_lines.clone();

        tokio::spawn(async move {
            loop {
                let mut line_handles_mg = gpio_lines.lock().await;
                let line_handle = match line_handles_mg.get(&request.pin) {
                    Some(v) => v,
                    None => {
                        let line_handle = get_new_line_handle(gpio_chip.lock().await, request.pin);
                        line_handles_mg.insert(request.pin, line_handle);
                        line_handles_mg.get(&request.pin).unwrap()
                    }
                };

                let value = line_handle.get_value().unwrap() == 1;

                drop(line_handles_mg); // release lock

                if let Err(e) = tx.send(Ok(GpioReadResponse { value: value })).await {
                    log::warn!("failed to send gpio value to client: {:?}", e);
                    break;
                }

                log::info!("gpio {} - value {} sent to client", request.pin, value);
                tokio::time::delay_for(Duration::from_millis(request.interval_ms)).await;
            }
        });
        Ok(Response::new(rx))
    }

    async fn write_gpio(
        &self,
        request: Request<tonic::Streaming<GpioWriteRequest>>,
    ) -> Result<Response<GpioWriteResponse>, Status> {
        let mut stream = request.into_inner();

        // The first message contains the pin
        let write_gpio_request: GpioWriteRequest = stream.next().await.unwrap()?;
        let pin = match write_gpio_request.request_type.expect("malformed request") {
            gpio_write_request::RequestType::Pin(pin) => pin,
            e => panic!("expected pin number, got {:?}", e),
        };

        let gpio_chip = self.gpio_chip.clone();
        let gpio_lines = self.gpio_lines.clone();

        while let Some(write_gpio_request) = stream.next().await {
            let mut line_handles_mg = gpio_lines.lock().await;
            let line_handle = match line_handles_mg.get(&pin) {
                Some(v) => v,
                None => {
                    let line_handle = get_new_line_handle(gpio_chip.lock().await, pin);
                    line_handles_mg.insert(pin, line_handle);
                    line_handles_mg.get(&pin).unwrap()
                }
            };

            let write_gpio_request: GpioWriteRequest = write_gpio_request?;

            let value = match write_gpio_request.request_type.expect("malformed request") {
                gpio_write_request::RequestType::Value(value) => value,
                _ => panic!("expected pin number"),
            };

            line_handle
                .set_value(value as u8)
                .expect("cannot operate gpio pin");

            drop(line_handles_mg); // release lock

            log::info!("gpio {} - value set to {}", pin, value);
        }

        Ok(Response::new(GpioWriteResponse {}))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::from_env(Env::default().default_filter_or("info")).init();

    let addr = "[::1]:2929".parse().unwrap();

    log::info!("Gpio Server listening on: {}", addr);

    let remote_gpio_service = RemoteGpio::new("gpiochip0");

    let svc = RemoteGpioServiceServer::new(remote_gpio_service);

    Server::builder().add_service(svc).serve(addr).await?;

    Ok(())
}
