-- MMORPG 数据库 Schema
-- PostgreSQL 16

CREATE TABLE IF NOT EXISTS players (
    uid BIGINT PRIMARY KEY,
    name VARCHAR(64) NOT NULL,
    level INT NOT NULL DEFAULT 1,
    exp BIGINT NOT NULL DEFAULT 0,
    x REAL NOT NULL DEFAULT 400,
    y REAL NOT NULL DEFAULT 300,
    hp INT NOT NULL DEFAULT 100,
    max_hp INT NOT NULL DEFAULT 100,
    mp INT NOT NULL DEFAULT 50,
    max_mp INT NOT NULL DEFAULT 50,
    atk INT NOT NULL DEFAULT 20,
    def INT NOT NULL DEFAULT 5,
    gold INT NOT NULL DEFAULT 0,
    weapon INT,     -- item_id
    armor INT,
    accessory INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS inventory (
    id SERIAL PRIMARY KEY,
    uid BIGINT NOT NULL REFERENCES players(uid) ON DELETE CASCADE,
    item_id INT NOT NULL,
    count INT NOT NULL DEFAULT 1,
    UNIQUE(uid, item_id)
);

CREATE TABLE IF NOT EXISTS skills (
    uid BIGINT NOT NULL REFERENCES players(uid) ON DELETE CASCADE,
    skill_id INT NOT NULL,
    level INT NOT NULL DEFAULT 1,
    PRIMARY KEY (uid, skill_id)
);

CREATE TABLE IF NOT EXISTS quests (
    uid BIGINT NOT NULL REFERENCES players(uid) ON DELETE CASCADE,
    quest_id INT NOT NULL,
    progress INT NOT NULL DEFAULT 0,
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (uid, quest_id)
);

CREATE INDEX IF NOT EXISTS idx_inventory_uid ON inventory(uid);
CREATE INDEX IF NOT EXISTS idx_quests_uid ON quests(uid);
CREATE INDEX IF NOT EXISTS idx_players_updated ON players(updated_at);
