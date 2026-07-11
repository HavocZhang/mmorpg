// 数据库持久化模块 — 支持 PostgreSQL (主) 和 SQLite (降级)
use sqlx::Row;
use tracing::{info, warn};

/// 数据库后端
pub enum Database {
    Postgres(sqlx::postgres::PgPool),
    Sqlite(sqlx::sqlite::SqlitePool),
}

impl Database {
    /// 尝试连接 PostgreSQL，失败则降级到 SQLite
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        // 先尝试 PostgreSQL
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            match sqlx::postgres::PgPool::connect(url).await {
                Ok(pool) => {
                    info!("✅ PostgreSQL 连接成功");
                    return Ok(Self::Postgres(pool));
                }
                Err(e) => {
                    warn!("⚠️ PostgreSQL 连接失败，降级到 SQLite: {}", e);
                }
            }
        }

        // 降级到 SQLite
        let sqlite_path = "mmo_game.db";
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(4)
            .connect(&format!("sqlite://{}?mode=rwc", sqlite_path))
            .await?;

        // 初始化 SQLite schema
        Self::init_sqlite_schema(&pool).await?;
        info!("✅ SQLite 降级模式启动: {}", sqlite_path);

        Ok(Self::Sqlite(pool))
    }

    /// 初始化 SQLite schema
    async fn init_sqlite_schema(pool: &sqlx::sqlite::SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS players (
                uid INTEGER PRIMARY KEY,
                name TEXT NOT NULL DEFAULT 'Player',
                level INTEGER NOT NULL DEFAULT 1,
                exp INTEGER NOT NULL DEFAULT 0,
                x REAL NOT NULL DEFAULT 400.0,
                y REAL NOT NULL DEFAULT 300.0,
                hp INTEGER NOT NULL DEFAULT 100,
                max_hp INTEGER NOT NULL DEFAULT 100,
                mp INTEGER NOT NULL DEFAULT 50,
                max_mp INTEGER NOT NULL DEFAULT 50,
                atk INTEGER NOT NULL DEFAULT 20,
                def INTEGER NOT NULL DEFAULT 5,
                weapon INTEGER,
                armor INTEGER,
                accessory INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS inventory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uid INTEGER NOT NULL,
                item_id INTEGER NOT NULL,
                count INTEGER NOT NULL DEFAULT 1,
                UNIQUE(uid, item_id),
                FOREIGN KEY (uid) REFERENCES players(uid)
            )",
        )
        .execute(pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS quests (
                uid INTEGER NOT NULL,
                quest_id INTEGER NOT NULL,
                progress INTEGER NOT NULL DEFAULT 0,
                completed INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (uid, quest_id),
                FOREIGN KEY (uid) REFERENCES players(uid)
            )",
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 加载玩家数据
    pub async fn load_player(&self, uid: u64) -> Result<Option<serde_json::Value>, sqlx::Error> {
        match self {
            Self::Postgres(pool) => self.load_player_pg(pool, uid).await,
            Self::Sqlite(pool) => self.load_player_sqlite(pool, uid).await,
        }
    }

    async fn load_player_pg(
        &self,
        pool: &sqlx::postgres::PgPool,
        uid: u64,
    ) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM players WHERE uid = $1")
            .bind(uid as i64)
            .fetch_optional(pool)
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

        let inv_rows = sqlx::query("SELECT item_id, count FROM inventory WHERE uid = $1")
            .bind(uid as i64)
            .fetch_all(pool)
            .await?;
        let items: Vec<serde_json::Value> = inv_rows.iter().map(|r| serde_json::json!({
            "itemId": r.get::<i32, _>("item_id"),
            "count": r.get::<i32, _>("count"),
        })).collect();

        let quest_rows = sqlx::query("SELECT quest_id, progress FROM quests WHERE uid = $1 AND completed = false")
            .bind(uid as i64)
            .fetch_all(pool)
            .await?;
        let quests: Vec<serde_json::Value> = quest_rows.iter().map(|r| serde_json::json!({
            "questId": r.get::<i32, _>("quest_id"),
            "progress": r.get::<i32, _>("progress"),
        })).collect();

        let mut result = player;
        result["inventory"] = serde_json::json!(items);
        result["quests"] = serde_json::json!(quests);

        let weapon: Option<i32> = row.get("weapon");
        let armor: Option<i32> = row.get("armor");
        let accessory: Option<i32> = row.get("accessory");
        result["weapon"] = serde_json::json!(weapon);
        result["armor"] = serde_json::json!(armor);
        result["accessory"] = serde_json::json!(accessory);

        Ok(Some(result))
    }

    async fn load_player_sqlite(
        &self,
        pool: &sqlx::sqlite::SqlitePool,
        uid: u64,
    ) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM players WHERE uid = ?1")
            .bind(uid as i64)
            .fetch_optional(pool)
            .await?;

        let Some(row) = row else { return Ok(None); };

        let player = serde_json::json!({
            "uid": uid,
            "name": row.get::<String, _>("name"),
            "level": row.get::<i32, _>("level"),
            "exp": row.get::<i64, _>("exp") as u64,
            "x": row.get::<f64, _>("x"),
            "y": row.get::<f64, _>("y"),
            "hp": row.get::<i32, _>("hp"),
            "maxHp": row.get::<i32, _>("max_hp"),
            "mp": row.get::<i32, _>("mp"),
            "maxMp": row.get::<i32, _>("max_mp"),
            "atk": row.get::<i32, _>("atk"),
            "def": row.get::<i32, _>("def"),
        });

        let inv_rows = sqlx::query("SELECT item_id, count FROM inventory WHERE uid = ?1")
            .bind(uid as i64)
            .fetch_all(pool)
            .await?;
        let items: Vec<serde_json::Value> = inv_rows.iter().map(|r| serde_json::json!({
            "itemId": r.get::<i32, _>("item_id"),
            "count": r.get::<i32, _>("count"),
        })).collect();

        let quest_rows = sqlx::query("SELECT quest_id, progress FROM quests WHERE uid = ?1 AND completed = 0")
            .bind(uid as i64)
            .fetch_all(pool)
            .await?;
        let quests: Vec<serde_json::Value> = quest_rows.iter().map(|r| serde_json::json!({
            "questId": r.get::<i32, _>("quest_id"),
            "progress": r.get::<i32, _>("progress"),
        })).collect();

        let mut result = player;
        result["inventory"] = serde_json::json!(items);
        result["quests"] = serde_json::json!(quests);

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
        match self {
            Self::Postgres(pool) => self.save_player_pg(pool, uid, data).await,
            Self::Sqlite(pool) => self.save_player_sqlite(pool, uid, data).await,
        }
    }

    async fn save_player_pg(
        &self,
        pool: &sqlx::postgres::PgPool,
        uid: u64,
        data: &serde_json::Value,
    ) -> Result<(), sqlx::Error> {
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
        .execute(pool)
        .await?;

        if let Some(items) = data["inventory"].as_array() {
            sqlx::query("DELETE FROM inventory WHERE uid = $1")
                .bind(uid_i64).execute(pool).await?;
            for item in items {
                sqlx::query("INSERT INTO inventory (uid, item_id, count) VALUES ($1,$2,$3)")
                    .bind(uid_i64)
                    .bind(item["itemId"].as_i64().unwrap_or(0) as i32)
                    .bind(item["count"].as_i64().unwrap_or(1) as i32)
                    .execute(pool).await?;
            }
        }

        if let Some(quests) = data["quests"].as_array() {
            sqlx::query("DELETE FROM quests WHERE uid = $1")
                .bind(uid_i64).execute(pool).await?;
            for q in quests {
                sqlx::query("INSERT INTO quests (uid, quest_id, progress) VALUES ($1,$2,$3)")
                    .bind(uid_i64)
                    .bind(q["questId"].as_i64().unwrap_or(0) as i32)
                    .bind(q["progress"].as_i64().unwrap_or(0) as i32)
                    .execute(pool).await?;
            }
        }

        Ok(())
    }

    async fn save_player_sqlite(
        &self,
        pool: &sqlx::sqlite::SqlitePool,
        uid: u64,
        data: &serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        let uid_i64 = uid as i64;
        let weapon: Option<i32> = data["weapon"].as_i64().map(|v| v as i32).or(data["weapon"].as_u64().map(|v| v as i32));
        let armor: Option<i32> = data["armor"].as_i64().map(|v| v as i32).or(data["armor"].as_u64().map(|v| v as i32));
        let accessory: Option<i32> = data["accessory"].as_i64().map(|v| v as i32).or(data["accessory"].as_u64().map(|v| v as i32));

        sqlx::query(
            "INSERT INTO players (uid, name, level, exp, x, y, hp, max_hp, mp, max_mp, atk, def, weapon, armor, accessory, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,CURRENT_TIMESTAMP)
             ON CONFLICT (uid) DO UPDATE SET
               name=?2, level=?3, exp=?4, x=?5, y=?6, hp=?7, max_hp=?8, mp=?9, max_mp=?10, atk=?11, def=?12,
               weapon=?13, armor=?14, accessory=?15, updated_at=CURRENT_TIMESTAMP"
        )
        .bind(uid_i64)
        .bind(data["name"].as_str().unwrap_or("Player"))
        .bind(data["level"].as_i64().unwrap_or(1) as i32)
        .bind(data["exp"].as_i64().unwrap_or(0))
        .bind(data["x"].as_f64().unwrap_or(400.0))
        .bind(data["y"].as_f64().unwrap_or(300.0))
        .bind(data["hp"].as_i64().unwrap_or(100) as i32)
        .bind(data["maxHp"].as_i64().unwrap_or(100) as i32)
        .bind(data["mp"].as_i64().unwrap_or(50) as i32)
        .bind(data["maxMp"].as_i64().unwrap_or(50) as i32)
        .bind(data["atk"].as_i64().unwrap_or(20) as i32)
        .bind(data["def"].as_i64().unwrap_or(5) as i32)
        .bind(weapon)
        .bind(armor)
        .bind(accessory)
        .execute(pool)
        .await?;

        if let Some(items) = data["inventory"].as_array() {
            sqlx::query("DELETE FROM inventory WHERE uid = ?1")
                .bind(uid_i64).execute(pool).await?;
            for item in items {
                sqlx::query("INSERT INTO inventory (uid, item_id, count) VALUES (?1,?2,?3)")
                    .bind(uid_i64)
                    .bind(item["itemId"].as_i64().unwrap_or(0) as i32)
                    .bind(item["count"].as_i64().unwrap_or(1) as i32)
                    .execute(pool).await?;
            }
        }

        if let Some(quests) = data["quests"].as_array() {
            sqlx::query("DELETE FROM quests WHERE uid = ?1")
                .bind(uid_i64).execute(pool).await?;
            for q in quests {
                sqlx::query("INSERT INTO quests (uid, quest_id, progress) VALUES (?1,?2,?3)")
                    .bind(uid_i64)
                    .bind(q["questId"].as_i64().unwrap_or(0) as i32)
                    .bind(q["progress"].as_i64().unwrap_or(0) as i32)
                    .execute(pool).await?;
            }
        }

        Ok(())
    }

    /// 获取数据库后端类型
    pub fn backend_name(&self) -> &'static str {
        match self {
            Self::Postgres(_) => "PostgreSQL",
            Self::Sqlite(_) => "SQLite",
        }
    }
}
