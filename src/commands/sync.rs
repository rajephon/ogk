#![allow(unused_must_use)]

use crate::client::{self, BillReturnType};
use crate::database::create_bills;
use crate::database::supabase::Supabase;
use crate::utils::log;
use crate::utils::progress;
use clap::Args;
use futures::future::join_all;
use indicatif::{HumanDuration, ProgressBar};
use std::error::Error;
use std::time::Instant;

#[derive(Args, Debug)]
pub struct Commands {
  #[clap(long = "from", required = true)]
  from: String,
  #[clap(long = "to", required = true)]
  to: String,
  #[clap(long = "with-slack-notification", required = false)]
  with_slack: Option<bool>,
}

pub async fn run(args: &Commands) -> Result<(), Box<dyn Error>> {
  let mut print_type = log::PrintType::DEFAULT;

  match &args.with_slack {
    Some(_with_slack) => {
      if *_with_slack == true {
        print_type = log::PrintType::SLACK;
      }
    }
    _ => {}
  }

  let started = Instant::now();
  let mut client = client::Client::new();
  client.auth_from_storage().await?;

  let init_page = 1 as i32;
  let init_count = 1 as i32;

  log::print(
    &format!(
      "SYNC [1/3] {}~{} {}청구 내역을 확인합니다.",
      &args.from,
      &args.to,
      progress::LOOKING_GLASS
    ),
    &print_type,
  )
  .await;

  let response = match client
    .fetch_bills(&init_page, &args.from, &args.to, &init_count)
    .await
  {
    Ok(_response) => _response,
    Err(e) => {
      eprintln!("{}", e);
      panic!();
    }
  };

  let total_count = &response.vo.totalPage;
  let pb = ProgressBar::new(*total_count as u64);

  log::print(
    &format!(
      "SYNC [2/3] {}청구 내역 {}건을 조회합니다.",
      progress::TRUCK,
      total_count
    ),
    &print_type,
  )
  .await;

  match client
    .fetch_bills(&init_page, &args.from, &args.to, total_count)
    .await
  {
    Ok(response) => {
      let mut bills: Vec<client::DtlVo> = vec![];
      let supabase_client = Supabase::new();

      let mut i = 0;
      let once_loop_len = 30;
      let mut is_last_index = false;

      while i < response.list.len() {
        let mut end_index = i + once_loop_len;
        if response.list.len() < i + once_loop_len {
          end_index = response.list.len();
          is_last_index = true;
        }

        pb.inc((end_index - i) as u64);

        let fetch_bills_awaits = response.list[i..end_index]
          .iter()
          .map(|bill| client.fetch_a_bill(bill));

        let results = join_all(fetch_bills_awaits).await;
        for bill in results {
          match bill {
            Ok(b) => {
              match b {
                BillReturnType::BillWithFiles(res) => {
                  bills.push(res.dtlVo);
                }
                BillReturnType::None => {
                  eprintln!("error2가 발생했습니다.")
                }
                _ => {}
              };
            }
            Err(e) => {
              eprintln!("{}", e);
              panic!();
            }
          }
        }

        if is_last_index == true {
          i = end_index;
        } else {
          i += once_loop_len;
        }
      }

      pb.finish_and_clear();

      log::print(
        &format!(
          "SYNC [3/3] {}조회한 내역을 데이터베이스에 저장합니다.",
          progress::DISK,
        ),
        &print_type,
      )
      .await;

      let _result = create_bills(&supabase_client, &bills).await;

      log::print(
        &format!(
          "SYNC {} 총 {}건 동기화 완료! - {}",
          progress::SPARKLE,
          total_count,
          HumanDuration(started.elapsed())
        ),
        &print_type,
      )
      .await;
    }
    Err(e) => {
      eprintln!("{}", e);
    }
  };

  Ok(())
}
