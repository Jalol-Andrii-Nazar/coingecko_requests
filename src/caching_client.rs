use std::collections::HashMap;

use directories::ProjectDirs;
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use futures::TryStreamExt;
use crate::{data, api_client};

#[derive(Debug)]
pub struct Client {
    api_client: api_client::Client,
    conn: SqlitePool
}

impl Client {
    pub async fn new(api_client: api_client::Client) -> Result<Self, Box<dyn std::error::Error>> {
        let project_dirs = ProjectDirs::from("org", "jna", "coingecko_requests")
            .ok_or::<Box<dyn std::error::Error>>(From::from("Failed to get project_dirs!"))?;
        let data_dir = project_dirs.data_dir().to_path_buf();
        tokio::fs::create_dir_all(&data_dir).await?;
        let mut db_path = data_dir;
        db_path.push("data");
        db_path.set_extension("db");
        println!("DB path: {:?}", db_path);
        tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&db_path)
            .await?;
        let db_path_str = db_path.to_str()
            .ok_or::<Box<dyn std::error::Error>>(From::from("db_path cannot be converted to str!"))?;
        let connection_url = format!("sqlite:{}", db_path_str);
        let conn: SqlitePool = SqlitePool::connect(&connection_url).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS vs_currencies (
                rowid INTEGER PRIMARY KEY, 
                name TEXT, 
                favourite BOOL)"
            )
            .execute(&conn)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS coins (
                rowid INTEGER PRIMARY KEY, 
                id TEXT, 
                symbol TEXT, 
                name TEXT, 
                favourite BOOL)"
            )
            .execute(&conn)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS market_chart_range_meta (
                rowid INTEGER PRIMARY KEY, 
                id TEXT, 
                currency TEXT, 
                from_timestamp INTEGER, 
                to_timestamp INTEGER)"
            )
            .execute(&conn)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS market_chart_range_prices (
                parent_rowid INTEGER, 
                timestamp INTEGER, 
                value REAL, 
                CONSTRAINT market_chart_range_prices__parent_rowid_fk FOREIGN KEY (parent_rowid) REFERENCES market_chart_range_meta (rowid))"
            )
            .execute(&conn)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS market_chart_range_market_caps (
                parent_rowid INTEGER, 
                timestamp INTEGER, 
                value REAL, 
                CONSTRAINT market_chart_range_market_caps__parent_rowid_fk FOREIGN KEY (parent_rowid) REFERENCES market_chart_range_meta (rowid))"
            )
            .execute(&conn)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS market_chart_range_total_volumes (
                parent_rowid INTEGER, 
                timestamp INTEGER, 
                value REAL, 
                CONSTRAINT market_chart_range_total_volumes__parent_rowid_fk FOREIGN KEY (parent_rowid) REFERENCES market_chart_range_meta (rowid))"
            )
            .execute(&conn)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS triggers (
                rowid INTEGER PRIMARY KEY, 
                coin_id INTEGER, 
                currency_id INTEGER, 
                initial_price REAL, 
                target_price REAL,
                CONSTRAINT triggers__coin_id_fk FOREIGN KEY (coin_id) REFERENCES coins (rowid),
                CONSTRAINT triggers__currency_id_fk FOREIGN KEY (currency_id) REFERENCES vs_currencies (rowid))"
            )
            .execute(&conn)
            .await?;

        Ok(Self {
            api_client,
            conn
        })
    }

    pub async fn ping(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.api_client.ping().await?)
    }

    pub async fn price(&self, ids: &[&str], vs_currencies: &[&str]) -> Result<HashMap<String, HashMap<String, f64>>, Box<dyn std::error::Error>> {
        Ok(self.api_client.price(ids, vs_currencies).await?)
    }

    pub async fn vs_currencies(&self) -> Result<Vec<data::VsCurrency>, Box<dyn std::error::Error>> {
        let count: i64 = sqlx::query("SELECT COUNT(*) FROM vs_currencies")
            .fetch_one(&self.conn)
            .await?
            .try_get(0)?;
        if count <= 0 {
            self.populate_vs_currencies().await?;
        }
        let mut rows = sqlx::query("SELECT rowid, name, favourite FROM vs_currencies")
            .fetch(&self.conn);
        let mut vec = Vec::new();
        while let Some(row) = rows.try_next().await? {
            let rowid: i64 = row.try_get("rowid")?;
            let name: String = row.try_get("name")?;
            let favourite: bool = row.try_get("favourite")?;
            vec.push(data::VsCurrency {
                rowid,
                raw: data::RawVsCurrency {
                    name,
                },
                favourite
            });
        }
        Ok(vec)
    }

    pub async fn favourite_vs_currencies(&self) -> Result<Vec<data::VsCurrency>, Box<dyn std::error::Error>> {
        Ok(self.vs_currencies()
            .await?
            .into_iter()
            .filter(|vs_currency| vs_currency.favourite)
            .collect())
    }

    pub async fn set_favourite_vs_currency(&self, id: i64, is_favourite: bool) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("UPDATE vs_currencies SET favourite = ? WHERE rowid = ?")
            .bind(is_favourite)
            .bind(id)
            .execute(&self.conn)
            .await?;
        Ok(())
    }

    pub async fn coins(&self) -> Result<Vec<data::Coin>, Box<dyn std::error::Error>> {
        let count: i64 = sqlx::query("SELECT COUNT(*) FROM coins")
            .fetch_one(&self.conn)
            .await?
            .try_get(0)?;
        if count <= 0 {
            self.populate_coins().await?;
        }
        let mut rows = sqlx::query("SELECT rowid, id, symbol, name, favourite FROM coins")
            .fetch(&self.conn);
        let mut vec = Vec::new();
        while let Some(row) = rows.try_next().await? {
            let rowid: i64 = row.try_get("rowid")?;
            let id: String = row.try_get("id")?;
            let symbol: String = row.try_get("symbol")?;
            let name: String = row.try_get("name")?;
            let favourite: bool = row.try_get("favourite")?;
            vec.push(data::Coin {
                rowid,
                raw: data::RawCoin {
                    id,
                    symbol,
                    name,
                },
                favourite
            });
        }
        Ok(vec)
    }

    pub async fn favourite_coins(&self) -> Result<Vec<data::Coin>, Box<dyn std::error::Error>> {
        Ok(self.coins()
            .await?
            .into_iter()
            .filter(|coin| coin.favourite)
            .collect())
    }

    pub async fn set_favourite_coin(&self, id: i64, is_favourite: bool) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("UPDATE coins SET favourite = ? WHERE rowid = ?")
            .bind(is_favourite)
            .bind(id)
            .execute(&self.conn)
            .await?;
        Ok(())
    }

    pub async fn market_chart(&self, id: &str, currency: &str, from: u64, to: u64) -> Result<data::MarketChart, Box<dyn std::error::Error>> {
        let meta_rowid_opt: Option<i64> = sqlx::query("SELECT rowid FROM market_chart_range_meta WHERE id = ? AND currency = ? AND from_timestamp = ? AND to_timestamp = ?")
            .bind(id)
            .bind(currency)
            .bind(from as i64)
            .bind(to as i64)
            .fetch_optional(&self.conn)
            .await?
            .map(|row: SqliteRow| {
                let rowid_opt: Option<i64> = row.try_get("rowid").ok();
                rowid_opt
            })
            .flatten();
        
        let meta_rowid = match meta_rowid_opt {
            Some(it) => { it }
            None => { self.populate_market_chart_data(id, currency, from, to).await? }
        };

        let mut prices: Vec<(u128, f64)> = Vec::new();
        let mut prices_rows = sqlx::query("SELECT timestamp, value FROM market_chart_range_prices WHERE parent_rowid = ?")
            .bind(meta_rowid)
            .fetch(&self.conn);
        while let Some(row) = prices_rows.try_next().await? {
            let timestamp: i64 = row.try_get("timestamp")?;
            let timestamp: u128 = timestamp as u128;
            let value: f64 = row.try_get("value")?;
            prices.push((timestamp, value));
        }
        drop(prices_rows);
        let prices = prices;

        let mut market_caps: Vec<(u128, f64)> = Vec::new();
        let mut market_caps_rows = sqlx::query("SELECT timestamp, value FROM market_chart_range_market_caps WHERE parent_rowid = ?")
            .bind(meta_rowid)
            .fetch(&self.conn);
        while let Some(row) = market_caps_rows.try_next().await? {
            let timestamp: i64 = row.try_get("timestamp")?;
            let timestamp: u128 = timestamp as u128;
            let value: f64 = row.try_get("value")?;
            market_caps.push((timestamp, value));
        }
        drop(market_caps_rows);
        let market_caps = market_caps;

        let mut total_volumes: Vec<(u128, f64)> = Vec::new();
        let mut total_volumes_rows = sqlx::query("SELECT timestamp, value FROM market_chart_range_total_volumes WHERE parent_rowid = ?")
            .bind(meta_rowid)
            .fetch(&self.conn);
        while let Some(row) = total_volumes_rows.try_next().await? {
            let timestamp: i64 = row.try_get("timestamp")?;
            let timestamp: u128 = timestamp as u128;
            let value: f64 = row.try_get("value")?;
            total_volumes.push((timestamp, value));
        }
        drop(total_volumes_rows);
        let total_volumes = total_volumes;

        Ok(data::MarketChart {
            meta_rowid,
            raw: data::RawMarketChart {
                prices,
                market_caps,
                total_volumes,
            }
        })
    }
    

    async fn populate_vs_currencies(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Receiving vs currencies data from the CoinGecko API...");
        let default_favourite_vs_currencies = vec!["btc", "eth", "ltc", "usd", "eur", "cad", "aud", "jpy", "pln", "rub", "uah"];
        let data = self.api_client.vs_currencies().await?;
        for vs_currency in data.iter() {
            let name = &vs_currency.name;
            let is_favourite = default_favourite_vs_currencies.contains(&name.as_str());
            sqlx::query("INSERT INTO vs_currencies (name, favourite) VALUES (?, ?)")
                .bind(name)
                .bind(is_favourite)
                .execute(&self.conn)
                .await?;
        }
        Ok(())
    }

    async fn populate_coins(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Receiving coins data from the CoinGecko API...");
        let default_favourite_coins = vec!["btc", "ltc", "eth", "doge", "xmr"];
        let data = self.api_client.coins().await?;
        for coin in data.iter() {
            let is_favourite = default_favourite_coins.contains(&coin.symbol.as_str());
            sqlx::query("INSERT INTO coins (id, symbol, name, favourite) VALUES (?, ?, ?, ?)")
                .bind(&coin.id)
                .bind(&coin.symbol)
                .bind(&coin.name)
                .bind(is_favourite)
                .execute(&self.conn)
                .await?;
        }
        Ok(())
    }

    async fn populate_market_chart_data(&self, id: &str, currency: &str, from: u64, to: u64) -> Result<i64, Box<dyn std::error::Error>> {
        println!("Receiving market chart data from the CoinGecko API...");
        let data = self.api_client.market_chart(id, currency, from, to).await?;
        let meta_rowid = sqlx::query("INSERT INTO market_chart_range_meta (id, currency, from_timestamp, to_timestamp) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(currency)
            .bind(from as i64)
            .bind(to as i64)
            .execute(&self.conn)
            .await?
            .last_insert_rowid();
        
        for (price_timestamp, price_value) in data.prices.iter() {
            sqlx::query("INSERT INTO market_chart_range_prices (parent_rowid, timestamp, value) VALUES (?, ?, ?)")
                .bind(meta_rowid)
                .bind(*price_timestamp as i64)
                .bind(*price_value)
                .execute(&self.conn)
                .await?;
        }
        
        for (market_cap_timestamp, market_cap_value) in data.prices.iter() {
            sqlx::query("INSERT INTO market_chart_range_market_caps (parent_rowid, timestamp, value) VALUES (?, ?, ?)")
                .bind(meta_rowid)
                .bind(*market_cap_timestamp as i64)
                .bind(*market_cap_value)
                .execute(&self.conn)
                .await?;
        }
        
        for (total_volume_timestamp, total_volume_value) in data.prices.iter() {
            sqlx::query("INSERT INTO market_chart_range_total_volumes (parent_rowid, timestamp, value) VALUES (?, ?, ?)")
                .bind(meta_rowid)
                .bind(*total_volume_timestamp as i64)
                .bind(*total_volume_value)
                .execute(&self.conn)
                .await?;
        }
        
        Ok(meta_rowid)
    }

    pub async fn add_trigger(&self, coin_id: i64, currency_id: i64, initial_price: f64, target_price: f64) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO triggers (coin_id, currency_id, initial_price, target_price) VALUES (?, ?, ?, ?)")
            .bind(coin_id)
            .bind(currency_id)
            .bind(initial_price)
            .bind(target_price)
            .execute(&self.conn)
            .await?;
        Ok(())
    }

    pub async fn delete_trigger(&self, rowid: i64) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("DELETE FROM triggers WHERE rowid = ?")
            .bind(rowid)
            .execute(&self.conn)
            .await?;
        Ok(())
    }

    pub async fn get_all_triggers(&self) -> Result<Vec<data::Trigger>, Box<dyn std::error::Error>> {
        let mut rows = sqlx::query("SELECT rowid, coin_id, currency_id, initial_price, target_price FROM triggers")
            .fetch(&self.conn);
        let mut vec: Vec<data::Trigger> = Vec::new();
        while let Some(row) = rows.try_next().await? {
            let rowid: i64 = row.try_get("rowid")?;
            let coin_id: i64 = row.try_get("coin_id")?;
            let currency_id: i64 = row.try_get("currency_id")?;
            let initial_price: f64 = row.try_get("initial_price")?;
            let target_price: f64 = row.try_get("target_price")?;
            vec.push(data::Trigger {
                rowid,
                coin_id,
                currency_id,
                initial_price,
                target_price
            })
        }
        Ok(vec)
    }

}