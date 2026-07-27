use reqwest::Client;
use serde_json::Value;

use super::UnlockItem;

pub(super) async fn check_bilibili_china_mainland(client: &Client) -> UnlockItem {
    let url = "https://api.bilibili.com/pgc/player/web/playurl?avid=82846771&qn=0&type=&otype=json&ep_id=307247&fourk=1&fnver=0&fnval=16&module=bangumi";

    match client.get(url).send().await {
        Ok(response) => match response.json::<Value>().await {
            Ok(body) => {
                let status = body
                    .get("code")
                    .and_then(|v| v.as_i64())
                    .map(|code| {
                        if code == 0 {
                            "Yes"
                        } else if code == -10403 {
                            "No"
                        } else {
                            "Failed"
                        }
                    })
                    .unwrap_or("Failed");

                UnlockItem::checked("哔哩哔哩大陆", status, None)
            }
            Err(_) => UnlockItem::checked("哔哩哔哩大陆", "Failed", None),
        },
        Err(_) => UnlockItem::checked("哔哩哔哩大陆", "Failed", None),
    }
}
