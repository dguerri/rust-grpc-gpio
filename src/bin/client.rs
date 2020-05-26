use std::error::Error;

use env_logger::{self, Env};
use log;
use std::time::Duration;
use tonic::transport::Channel;
use tonic::Request;

use remote_gpio_service::remote_gpio_service_client::RemoteGpioServiceClient;
use remote_gpio_service::{gpio_write_request, GpioReadRequest, GpioWriteRequest};

pub mod remote_gpio_service {
    tonic::include_proto!("remote_gpio_service");
}

async fn demo_read_gpio(
    client: &mut RemoteGpioServiceClient<Channel>,
    pin: u32,
    interval_ms: u64,
    times: u64,
) -> Result<(), Box<dyn Error>> {
    log::info!("demo_read_gpio");

    let mut stream = client
        .read_gpio(Request::new(GpioReadRequest {
            pin: pin,
            interval_ms: interval_ms,
        }))
        .await?
        .into_inner();

    let mut t: u64 = times;
    while let Some(response) = stream.message().await? {
        log::info!("Read - gpio {} value: {}", pin, response.value);
        if times > 0 {
            t -= 1;
            if t == 0 {
                break;
            }
        }
    }

    Ok(())
}

async fn demo_write_gpio(
    client: &mut RemoteGpioServiceClient<Channel>,
    pin: u32,
    interval_ms: u64,
    times: u64,
) -> Result<(), Box<dyn Error>> {
    log::info!("demo_write_gpio");

    let mut t: u64 = times;
    let outbound = async_stream::stream! {
        let gpio_write_request = GpioWriteRequest {
            request_type: Some(gpio_write_request::RequestType::Pin(pin))
        };
        yield gpio_write_request;

        loop {
            let gpio_write_request = GpioWriteRequest {
                request_type: Some(gpio_write_request::RequestType::Value((t % 2 == 0)))
            };
            log::info!("Write - gpio {} value: {}", pin, (t % 2 == 0));
            yield gpio_write_request;

            tokio::time::delay_for(Duration::from_millis(interval_ms)).await;

            if times > 0 {
                t -= 1;
                if t == 0 {
                    break;
                }
            }
        }
    };

    match client.write_gpio(Request::new(outbound)).await {
        Ok(_response) => log::info!("all good"),
        Err(e) => log::info!("something went wrong: {:?}", e),
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::from_env(Env::default().default_filter_or("info")).init();

    let mut client = RemoteGpioServiceClient::connect("http://[::1]:2929").await?;

    demo_write_gpio(&mut client, 21, 100, 15).await?;
    demo_read_gpio(&mut client, 26, 10, 0).await?;

    Ok(())
}
