/**
 * MMORPG 端到端测试脚本
 *
 * 通过 WebSocket 代理连接两个玩家，验证完整消息闭环：
 * 1. 玩家上线 → 收到属性(5001) + 进入广播(8002) + 玩家列表(9001)
 * 2. 聊天 → ACK(7001) + 广播(7002)
 * 3. 移动 → 位置更新(8001)
 * 4. 查询 → 玩家列表(9001)
 * 5. 攻击 → 战斗结果(6001)
 * 6. 玩家离线 → 离开广播(8003)
 */

const WebSocket = require("ws");

const PROXY_URL = "ws://localhost:3000/ws";

// 消息ID常量
const MSG = {
  INIT: 100,
  ATTACK: 1001,
  CHAT: 2001,
  MOVE: 3001,
  QUERY: 4001,
  STATS: 5001,
  BATTLE: 6001,
  CHAT_ACK: 7001,
  CHAT_BROADCAST: 7002,
  POSITION_UPDATE: 8001,
  PLAYER_ENTER: 8002,
  PLAYER_LEAVE: 8003,
  PLAYER_LIST: 9001,
};

const MSG_NAMES = {};
Object.entries(MSG).forEach(([k, v]) => (MSG_NAMES[v] = k));

// 测试结果收集
let passed = 0;
let failed = 0;
const results = [];

function log(msg) {
  console.log(`  ${msg}`);
}

function ok(desc) {
  passed++;
  results.push({ test: desc, status: "PASS" });
  console.log(`  \x1b[32m[PASS]\x1b[0m ${desc}`);
}

function fail(desc, reason) {
  failed++;
  results.push({ test: desc, status: "FAIL", reason });
  console.log(`  \x1b[31m[FAIL]\x1b[0m ${desc} — ${reason}`);
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * 创建一个 WebSocket 客户端连接到代理
 */
class TestClient {
  constructor(name, uid) {
    this.name = name;
    this.uid = uid;
    this.ws = null;
    this.connId = `${name}-${uid}`;
    this.messages = []; // 收到的所有消息
    this.connected = false;
    this.ready = false;
  }

  connect() {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(PROXY_URL);
      const timeout = setTimeout(() => reject(new Error("WS connect timeout")), 5000);

      this.ws.on("open", () => {
        clearTimeout(timeout);
        // Wait for "ready" event from proxy
      });

      this.ws.on("message", (raw) => {
        const msg = JSON.parse(raw.toString());

        if (msg.event === "ready") {
          this.ready = true;
          // Now send connect command to establish TCP connection to gateway
          this.ws.send(
            JSON.stringify({
              action: "connect",
              id: this.connId,
              uid: this.uid,
              token: "test_token_123",
              host: "127.0.0.1",
              port: 7888,
            })
          );
          resolve();
        } else if (msg.event === "connected") {
          this.connected = true;
          log(`${this.name}: TCP connected (uid=${msg.uid})`);
        } else if (msg.event === "connectFailed") {
          this.connected = false;
          log(`${this.name}: connect failed — ${msg.reason}`);
        } else if (msg.event === "message") {
          this.messages.push({
            msgId: msg.msgId,
            data: msg.data,
            timestamp: Date.now(),
          });
        } else if (msg.event === "disconnected") {
          this.connected = false;
          log(`${this.name}: disconnected — ${msg.reason}`);
        }
      });

      this.ws.on("error", (err) => {
        clearTimeout(timeout);
        reject(err);
      });
    });
  }

  send(msgId, data) {
    this.ws.send(
      JSON.stringify({
        action: "send",
        id: this.connId,
        msgId,
        data: typeof data === "string" ? data : JSON.stringify(data),
      })
    );
  }

  disconnect() {
    this.ws.send(
      JSON.stringify({
        action: "disconnect",
        id: this.connId,
      })
    );
  }

  close() {
    if (this.ws) this.ws.close();
  }

  // 等待指定 msgId 的消息，带超时
  waitForMsg(msgId, timeoutMs = 3000) {
    return new Promise((resolve) => {
      const start = Date.now();
      const check = () => {
        const found = this.messages.find((m) => m.msgId === msgId);
        if (found) {
          resolve(found);
        } else if (Date.now() - start > timeoutMs) {
          resolve(null);
        } else {
          setTimeout(check, 50);
        }
      };
      check();
    });
  }

  // 获取所有指定 msgId 的消息
  getMessages(msgId) {
    return this.messages.filter((m) => m.msgId === msgId);
  }

  // 等待至少 N 条指定 msgId 的消息
  waitForMsgCount(msgId, count, timeoutMs = 3000) {
    return new Promise((resolve) => {
      const start = Date.now();
      const check = () => {
        const found = this.messages.filter((m) => m.msgId === msgId);
        if (found.length >= count) {
          resolve(found);
        } else if (Date.now() - start > timeoutMs) {
          resolve(found);
        } else {
          setTimeout(check, 50);
        }
      };
      check();
    });
  }
}

async function runTests() {
  console.log("\n========================================");
  console.log("  MMORPG 端到端测试");
  console.log("========================================\n");

  // ── 1. 连接 Player 1 ──
  console.log("--- 阶段1: Player 1 连接 ---");
  const p1 = new TestClient("P1", 10001);
  try {
    await p1.connect();
    await sleep(500); // 等待 connected 事件
  } catch (e) {
    fail("Player 1 WebSocket 连接", e.message);
    return;
  }

  if (p1.connected) {
    ok("Player 1 TCP 连接成功");
  } else {
    fail("Player 1 TCP 连接", "未收到 connected 事件");
    return;
  }

  // 等待上线响应：5001(属性) + 8002(进入广播) + 9001(玩家列表)
  const p1_stats = await p1.waitForMsg(MSG.STATS);
  if (p1_stats) {
    const data = JSON.parse(p1_stats.data);
    ok(`Player 1 收到属性(5001): name=${data.name}, hp=${data.hp}/${data.maxHp}`);
  } else {
    fail("Player 1 收到属性(5001)", "超时未收到");
  }

  const p1_enter = await p1.waitForMsg(MSG.PLAYER_ENTER);
  if (p1_enter) {
    const data = JSON.parse(p1_enter.data);
    ok(`Player 1 收到进入通知(8002): uid=${data.uid}, name=${data.name}`);
  } else {
    fail("Player 1 收到进入通知(8002)", "超时未收到");
  }

  const p1_list = await p1.waitForMsg(MSG.PLAYER_LIST);
  if (p1_list) {
    ok("Player 1 收到玩家列表(9001)");
  } else {
    fail("Player 1 收到玩家列表(9001)", "超时未收到");
  }

  // ── 2. 连接 Player 2 ──
  console.log("\n--- 阶段2: Player 2 连接 ---");
  const p2 = new TestClient("P2", 10002);
  try {
    await p2.connect();
    await sleep(500);
  } catch (e) {
    fail("Player 2 WebSocket 连接", e.message);
    return;
  }

  if (p2.connected) {
    ok("Player 2 TCP 连接成功");
  } else {
    fail("Player 2 TCP 连接", "未收到 connected 事件");
    return;
  }

  // Player 2 也应收到属性 + 进入广播 + 玩家列表
  const p2_stats = await p2.waitForMsg(MSG.STATS);
  if (p2_stats) {
    ok("Player 2 收到属性(5001)");
  } else {
    fail("Player 2 收到属性(5001)", "超时未收到");
  }

  // Player 1 应收到 Player 2 的进入通知
  const p1_p2_enter = await p1.waitForMsg(MSG.PLAYER_ENTER, 2000);
  if (p1_p2_enter) {
    const data = JSON.parse(p1_p2_enter.data);
    ok(`Player 1 收到 Player 2 进入通知(8002): uid=${data.uid}`);
  } else {
    fail("Player 1 收到 Player 2 进入通知(8002)", "超时未收到");
  }

  // Player 2 应收到 Player 1 已在线的通知
  const p2_p1_enter = await p2.waitForMsg(MSG.PLAYER_ENTER, 2000);
  if (p2_p1_enter) {
    const data = JSON.parse(p2_p1_enter.data);
    ok(`Player 2 收到 Player 1 已在线通知(8002): uid=${data.uid}`);
  } else {
    fail("Player 2 收到 Player 1 已在线通知(8002)", "超时未收到");
  }

  // ── 3. 聊天测试 ──
  console.log("\n--- 阶段3: 聊天 ---");
  p2.send(MSG.CHAT, { text: "Hello from P2!" });
  await sleep(300);

  const p2_chat_ack = await p2.waitForMsg(MSG.CHAT_ACK, 2000);
  if (p2_chat_ack) {
    ok("Player 2 收到聊天ACK(7001)");
  } else {
    fail("Player 2 收到聊天ACK(7001)", "超时未收到");
  }

  const p1_chat_broadcast = await p1.waitForMsg(MSG.CHAT_BROADCAST, 2000);
  if (p1_chat_broadcast) {
    const data = JSON.parse(p1_chat_broadcast.data);
    ok(`Player 1 收到聊天广播(7002): from=${data.from}, text="${data.text}"`);
  } else {
    fail("Player 1 收到聊天广播(7002)", "超时未收到");
  }

  // ── 4. 移动测试 ──
  console.log("\n--- 阶段4: 移动 ---");
  p2.send(MSG.MOVE, { x: 250.5, y: 180.3, dir: 1 });
  await sleep(300);

  const p1_move = await p1.waitForMsg(MSG.POSITION_UPDATE, 2000);
  if (p1_move) {
    const data = JSON.parse(p1_move.data);
    ok(`Player 1 收到位置更新(8001): uid=${data.uid}, x=${data.x}, y=${data.y}, dir=${data.dir}`);
  } else {
    fail("Player 1 收到位置更新(8001)", "超时未收到");
  }

  // ── 5. 查询附近玩家 ──
  console.log("\n--- 阶段5: 查询附近玩家 ---");
  p2.send(MSG.QUERY, {});
  await sleep(300);

  const p2_query_result = await p2.waitForMsg(MSG.PLAYER_LIST, 2000);
  if (p2_query_result) {
    const data = JSON.parse(p2_query_result.data);
    const playerCount = data.players ? data.players.length : 0;
    ok(`Player 2 查询附近玩家(9001): 找到 ${playerCount} 个玩家`);
  } else {
    fail("Player 2 查询附近玩家(9001)", "超时未收到");
  }

  // ── 6. 战斗测试 ──
  console.log("\n--- 阶段6: 战斗 ---");
  p1.send(MSG.ATTACK, { targetUid: 10002 });
  await sleep(500);

  const p1_battle = await p1.waitForMsg(MSG.BATTLE, 2000);
  if (p1_battle) {
    const data = JSON.parse(p1_battle.data);
    ok(`Player 1 收到战斗结果(6001): targetUid=${data.targetUid}, dmg=${data.dmg}, targetHp=${data.targetHp}`);
  } else {
    fail("Player 1 收到战斗结果(6001)", "超时未收到");
  }

  const p2_battle = await p2.waitForMsg(MSG.BATTLE, 2000);
  if (p2_battle) {
    const data = JSON.parse(p2_battle.data);
    ok(`Player 2 收到被攻击通知(6001): attackerUid=${data.attackerUid}, dmg=${data.dmg}, hp=${data.hp}`);
  } else {
    fail("Player 2 收到被攻击通知(6001)", "超时未收到");
  }

  // ── 7. 玩家离线测试 ──
  console.log("\n--- 阶段7: Player 2 离线 ---");
  p2.disconnect();
  await sleep(500);

  const p1_leave = await p1.waitForMsg(MSG.PLAYER_LEAVE, 2000);
  if (p1_leave) {
    const data = JSON.parse(p1_leave.data);
    ok(`Player 1 收到 Player 2 离开通知(8003): uid=${data.uid}`);
  } else {
    fail("Player 1 收到 Player 2 离开通知(8003)", "超时未收到");
  }

  // ── 清理 ──
  p1.disconnect();
  await sleep(200);
  p1.close();
  p2.close();

  // ── 汇总 ──
  console.log("\n========================================");
  console.log(`  测试结果: ${passed} 通过, ${failed} 失败`);
  console.log("========================================\n");

  process.exit(failed > 0 ? 1 : 0);
}

// 运行测试
runTests().catch((e) => {
  console.error("测试异常:", e);
  process.exit(1);
});
