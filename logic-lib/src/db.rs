// 数据库持久化模块
use sqlx::postgres::PgPool;
use sqlx::Row;

pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(url).await?;
        Ok(Self { pool })
    }

    /// 加载玩家数据，不存在返回 None
    pub async fn load_player(&self, uid: u64) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM players WHERE uid = $1")
            .bind(uid as i64)
            .fetch_optional(&self.pool)
            .await?;

        let Some(row) = row else { return Ok(None); };

        let player = serde_json::json!({
            "uid": uid,
            "name": row.get::<String, _>("name"),
            "level": row.get::<i32, _>("level"),
            "exp": row.get::<i64, _>("exp") as u64,
            "x": row.get::<f32, _>("x"),
            "y": row.get::<f32, _>("y"),
            "hp": row.get::<i32, _>("hp"),
            "maxHp": row.get::<i32, _>("max_hp"),
            "mp": row.get::<i32, _>("mp"),
            "maxMp": row.get::<i32, _>("max_mp"),
            "atk": row.get::<i32, _>("atk"),
            "def": row.get::<i32, _>("def"),
        });

        // Load inventory
        let inv_rows = sqlx::query("SELECT item_id, count FROM inventory WHERE uid = $1")
            .bind(uid as i64)
            .fetch_all(&self.pool)
            .await?;
        let items: Vec<serde_json::Value> = inv_rows.iter().map(|r| serde_json::json!({
            "itemId": r.get::<i32, _>("item_id"),
            "count": r.get::<i32, _>("count"),
        })).collect();

        // Load quests
        let quest_rows = sqlx::query("SELECT quest_id, progress FROM quests WHERE uid = $1 AND completed = false")
            .bind(uid as i64)
            .fetch_all(&self.pool)
            .await?;
        let quests: Vec<serde_json::Value> = quest_rows.iter().map(|r| serde_json::json!({
            "questId": r.get::<i32, _>("quest_id"),
            "progress": r.get::<i32, _>("progress"),
        })).collect();

        let mut result = player;
        result["inventory"] = serde_json::json!(items);
        result["quests"] = serde_json::json!(quests);

        // Equipment
        let weapon: Option<i32> = row.get("weapon");
        let armor: Option<i32> = row.get("armor");
        let accessory: Option<i32> = row.get("accessory");
        result["weapon"] = serde_json::json!(weapon);
        result["armor"] = serde_json::json!(armor);
        result["accessory"] = serde_json::json!(accessory);

        Ok(Some(result))
    }

    /// 保存玩家数据 (upsert)
    pub async fn save_player(&self, uid: u64, data: &serde_json::Value) -> Result<(), sqlx::Error> {
        let uid_i64 = uid as i64;
        let weapon: Option<i32> = data["weapon"].as_i64().map(|v| v as i32).or(data["weapon"].as_u64().map(|v| v as i32));
        let armor: Option<i32> = data["armor"].as_i64().map(|v| v as i32).or(data["armor"].as_u64().map(|v| v as i32));
        let accessory: Option<i32> = data["accessory"].as_i64().map(|v| v as i32).or(data["accessory"].as_u64().map(|v| v as i32));

        sqlx::query(
            "INSERT INTO players (uid, name, level, exp, x, y, hp, max_hp, mp, max_mp, atk, def, weapon, armor, accessory, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,NOW())
             ON CONFLICT (uid) DO UPDATE SET
               name=$2, level=$3, exp=$4, x=$5, y=$6, hp=$7, max_hp=$8, mp=$9, max_mp=$10, atk=$11, def=$12,
               weapon=$13, armor=$14, accessory=$15, updated_at=NOW()"
        )
        .bind(uid_i64)
        .bind(data["name"].as_str().unwrap_or("Player"))
        .bind(data["level"].as_i64().unwrap_or(1) as i32)
        .bind(data["exp"].as_i64().unwrap_or(0))
        .bind(data["x"].as_f64().unwrap_or(400.0) as f32)
        .bind(data["y"].as_f64().unwrap_or(300.0) as f32)
        .bind(data["hp"].as_i64().unwrap_or(100) as i32)
        .bind(data["maxHp"].as_i64().unwrap_or(100) as i32)
        .bind(data["mp"].as_i64().unwrap_or(50) as i32)
        .bind(data["maxMp"].as_i64().unwrap_or(50) as i32)
        .bind(data["atk"].as_i64().unwrap_or(20) as i32)
        .bind(data["def"].as_i64().unwrap_or(5) as i32)
        .bind(weapon)
        .bind(armor)
        .bind(accessory)
        .execute(&self.pool)
        .await?;

        // Save inventory
        if let Some(items) = data["inventory"].as_array() {
            sqlx::query("DELETE FROM inventory WHERE uid = $1")
                .bind(uid_i64).execute(&self.pool).await?;
            for item in items {
                sqlx::query("INSERT INTO inventory (uid, item_id, count) VALUES ($1,$2,$3)")
                    .bind(uid_i64)
                    .bind(item["itemId"].as_i64().unwrap_or(0) as i32)
                    .bind(item["count"].as_i64().unwrap_or(1) as i32)
                    .execute(&self.pool).await?;
            }
        }

        // Save quests
        if let Some(quests) = data["quests"].as_array() {
            sqlx::query("DELETE FROM quests WHERE uid = $1")
                .bind(uid_i64).execute(&self.pool).await?;
            for q in quests {
                sqlx::query("INSERT INTO quests (uid, quest_id, progress) VALUES ($1,$2,$3)")
                    .bind(uid_i64)
                    .bind(q["questId"].as_i64().unwrap_or(0) as i32)
                    .bind(q["progress"].as_i64().unwrap_or(0) as i32)
                    .execute(&self.pool).await?;
            }
        }

        Ok(())
    }

    pub fn pool(&self) -> &PgPool { &self.pool }
}
