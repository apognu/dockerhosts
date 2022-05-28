use std::{collections::HashMap, env, error::Error, io::Write, process, sync::Arc, time::Duration};

use bollard::{
  models::{ContainerInspectResponse, EventMessageTypeEnum, NetworkSettings},
  system::EventsOptions,
  Docker,
};
use futures::stream::StreamExt;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
  if env::args().len() != 3 {
    eprintln!("Usage: dockerhosts FILE SUFFIX");
    process::exit(1);
  }

  let filename = Arc::new(env::args().nth(1).unwrap());
  let suffix = Arc::new(env::args().nth(2).unwrap().trim_start_matches('.').to_string());

  let docker = Arc::new(Docker::connect_with_unix_defaults()?);

  tokio::spawn({
    let docker = Arc::clone(&docker);
    let (filename, suffix) = (Arc::clone(&filename), Arc::clone(&suffix));

    async move {
      loop {
        let _ = inspect_containers(&docker, &filename, &suffix).await;

        tokio::time::sleep(Duration::from_secs(30)).await;
      }
    }
  });

  let mut events = docker.events(Some(EventsOptions::<String>::default()));

  while let Some(event) = events.next().await {
    if let Ok(event) = event {
      match (event.typ, event.action.as_deref()) {
        (Some(EventMessageTypeEnum::CONTAINER), Some("start")) | (Some(EventMessageTypeEnum::CONTAINER), Some("die")) => {
          let _ = inspect_containers(&docker, &filename, &suffix).await;
        }

        _ => {}
      }
    }
  }

  Ok(())
}

async fn inspect_containers(docker: &Docker, filename: &str, suffix: &str) -> Result<(), Box<dyn Error>> {
  let mut containers: HashMap<String, (String, String)> = HashMap::new();

  for container in docker.list_containers::<String>(None).await? {
    if let Some(id) = container.id {
      if let Some([name]) = container.names.as_deref() {
        if let Ok(ContainerInspectResponse {
          network_settings: Some(NetworkSettings { ip_address: Some(ip), .. }),
          ..
        }) = docker.inspect_container(&id, None).await
        {
          if !ip.is_empty() {
            containers.insert(id, (name.trim_start_matches('/').to_string(), ip));
          }
        }
      }
    }
  }

  write_hosts_file(&containers, filename, suffix).await?;

  Ok(())
}

async fn write_hosts_file(containers: &HashMap<String, (String, String)>, filename: &str, suffix: &str) -> Result<(), Box<dyn Error>> {
  let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(filename).await?;
  let mut buffer = Vec::<u8>::new();

  for (name, ip) in containers.values() {
    let _ = writeln!(buffer, "{ip} {name}.{suffix}");
  }

  file.write_all(&buffer).await?;

  eprintln!("Written {} containers to hosts file", containers.len());

  Ok(())
}
