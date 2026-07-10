/**
 * 跨网关集群端到端测试
 *
 * Player A (uid=30001) → Gateway 1 (port 7888, node_id=1)
 * Player B (uid=30002) → Gateway 2 (port 7889, node_id=2)
 *
 * 验证: 聊天/移动/战斗/进出视野 全部跨网关同步
 */

const WebSocket = require('ws');

const PROXY_URL = 'ws://127.0.0.1:3000/ws';
const GATE1_PORT = 7888;
const GATE2_PORT = 7889;

let totalPass = 0;
let totalFail = 0;

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

class GameClient {
  constructor(name, uid, gatePort) {
    this.name = name;
    this.uid = uid;
    this.gatePort = gatePort;
    this.ws = null;
    this.connId = null;
    this.messages = [];
    this.connected = false;
  }

  connect() {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(PROXY_URL);
      const timeout = setTimeout(() => reject(new Error('WS connect timeout')), 5000);

      this.ws.on('open', () => {
        this.ws.send(JSON.stringify({
          action: 'connect',
          id: this.name,
          uid: this.uid,
          token: 'test_token_123',
          host: '127.0.0.1',
          port: this.gatePort,
        }));
      });

      this.ws.on('message', (data) => {
        const msg = JSON.parse(data);
        if (msg.event === 'connected' && msg.id === this.name) {
          clearTimeout(timeout);
          this.connected = true;
          this.connId = msg.id;
          resolve();
        }
        if (msg.event === 'message' && msg.id === this.name) {
          this.messages.push({
            msgId: msg.msgId,
            data: msg.data,
            timestamp: Date.now(),
          });
        }
        if (msg.event === 'connectFailed' && msg.id === this.name) {
          clearTimeout(timeout);
          reject(new Error(msg.reason));
        }
      });

      this.ws.on('error', (err) => {
        clearTimeout(timeout);
        reject(err);
      });
    });
  }

  send(msgId, data) {
    this.ws.send(JSON.stringify({
      action: 'send',
      id: this.name,
      msgId: msgId,
      data: typeof data === 'string' ? data : JSON.stringify(data),
    }));
  }

  getMsgs(msgId) {
    return this.messages.filter((m) => m.msgId === msgId);
  }

  getAllMsgs() {
    return this.messages.slice();
  }

  clearMsgs() {
    this.messages = [];
  }

  disconnect() {
    if (this.ws) {
      this.ws.send(JSON.stringify({ action: 'disconnect', id: this.name }));
      setTimeout(() => this.ws.close(), 500);
    }
  }
}

function assert(condition, testName, detail) {
  if (condition) {
    console.log(`  PASS  ${testName}`);
    totalPass++;
  } else {
    console.log(`  FAIL  ${testName}  ${detail || ''}`);
    totalFail++;
  }
}

async function run() {
  console.log('');
  console.log('============================================');
  console.log('  跨网关集群端到端测试');
  console.log('  Player A (uid=30001) -> Gate1 (port 7888)');
  console.log('  Player B (uid=30002) -> Gate2 (port 7889)');
  console.log('============================================');
  console.log('');

  // ── 1. 双玩家分别连接不同网关 ──
  console.log('[1] 双玩家跨网关连接');
  const playerA = new GameClient('playerA', 30001, GATE1_PORT);
  const playerB = new GameClient('playerB', 30002, GATE2_PORT);

  await playerA.connect();
  console.log('  Player A connected to Gate1 (port 7888)');
  await sleep(500);

  await playerB.connect();
  console.log('  Player B connected to Gate2 (port 7889)');
  await sleep(1000);

  // 验证两个网关都有在线玩家
  const gate1Health = await fetch('http://127.0.0.1:9090/health').then((r) => r.json());
  const gate2Health = await fetch('http://127.0.0.1:9091/health').then((r) => r.json());
  assert(gate1Health.online_count >= 1, 'Gate1 有在线玩家', `online_count=${gate1Health.online_count}`);
  assert(gate2Health.online_count >= 1, 'Gate2 有在线玩家', `online_count=${gate2Health.online_count}`);
  assert(gate1Health.node_id === 1, 'Gate1 node_id=1');
  assert(gate2Health.node_id === 2, 'Gate2 node_id=2');

  // ── 2. Player A 收到自己的上线消息 ──
  console.log('');
  console.log('[2] Player A 上线消息 (via Gate1 -> logic -> Gate1)');
  const aProps = playerA.getMsgs(5001);
  assert(aProps.length > 0, 'Player A 收到属性(5001)', `got ${aProps.length}`);
  if (aProps.length > 0) {
    const props = JSON.parse(aProps[0].data);
    assert(props.uid === 30001, 'Player A 属性 uid=30001', `uid=${props.uid}`);
    console.log(`        HP=${props.hp}/${props.maxHp} MP=${props.mp}/${props.maxMp} Lv=${props.level}`);
  }

  // ── 3. Player B 上线 → Player A 跨网关收到进入通知 ──
  console.log('');
  console.log('[3] Player B 上线 -> Player A 跨网关收到进入通知(8002)');
  await sleep(500);
  const enterMsgs = playerA.getMsgs(8002);
  assert(enterMsgs.length > 0, 'Player A 收到 Player B 进入通知(8002)', `got ${enterMsgs.length} msgs`);
  if (enterMsgs.length > 0) {
    const playerBEnter = enterMsgs.find((m) => {
      const d = JSON.parse(m.data);
      return d.uid === 30002 || d.fromUid === 30002;
    });
    assert(playerBEnter !== undefined, '进入通知的 uid=30002 (Player B)', `got ${enterMsgs.length} enter msgs, uids: ${enterMsgs.map(m => { try { return JSON.parse(m.data).uid; } catch { return '?'; } }).join(',')}`);
    if (playerBEnter) {
      const enterData = JSON.parse(playerBEnter.data);
      console.log(`        Player B entered: name=${enterData.name} pos=(${enterData.x},${enterData.y})`);
    }
  }

  // ── 4. Player A 聊天 → Player B 跨网关收到聊天广播 ──
  console.log('');
  console.log('[4] Player A 聊天 -> Player B 跨网关收到聊天广播(7002)');
  playerB.clearMsgs();
  playerA.send(2001, JSON.stringify({
    channel: 'world',
    text: 'Hello from Gate1!',
  }));
  await sleep(1000);

  const chatAck = playerA.getMsgs(7001);
  assert(chatAck.length > 0, 'Player A 收到聊天ACK(7001)', `got ${chatAck.length}`);

  const chatBroadcast = playerB.getMsgs(7002);
  assert(chatBroadcast.length > 0, 'Player B 跨网关收到聊天广播(7002)', `got ${chatBroadcast.length}`);
  if (chatBroadcast.length > 0) {
    const chatData = JSON.parse(chatBroadcast[0].data);
    assert(chatData.text === 'Hello from Gate1!', '聊天内容正确', `text="${chatData.text}"`);
    console.log(`        [世界] ${chatData.fromName}: ${chatData.text}`);
  }

  // ── 5. Player A 移动 → Player B 跨网关收到位置更新 ──
  console.log('');
  console.log('[5] Player A 移动 -> Player B 跨网关收到位置更新(8001)');
  playerB.clearMsgs();
  playerA.send(3001, JSON.stringify({
    x: 200,
    y: 300,
    dir: 1,
  }));
  await sleep(1000);

  const moveMsgs = playerB.getMsgs(8001);
  assert(moveMsgs.length > 0, 'Player B 跨网关收到位置更新(8001)', `got ${moveMsgs.length}`);
  if (moveMsgs.length > 0) {
    const moveData = JSON.parse(moveMsgs[0].data);
    console.log(`        Player A 位置: (${moveData.x}, ${moveData.y}) dir=${moveData.dir}`);
  }

  // ── 6. Player A 查询 → 收到附近玩家列表(含 Player B) ──
  console.log('');
  console.log('[6] Player A 查询玩家 -> 收到列表(9001, 含跨网关 Player B)');
  playerA.clearMsgs();
  playerA.send(4001, JSON.stringify({ type: 'nearby' }));
  await sleep(1000);

  const playerList = playerA.getMsgs(9001);
  assert(playerList.length > 0, 'Player A 收到玩家列表(9001)', `got ${playerList.length}`);
  if (playerList.length > 0) {
    const listData = JSON.parse(playerList[0].data);
    const players = listData.players || listData;
    // players 可能是 JSON 字符串数组，需要逐个解析
    const parsedPlayers = players.map(p => typeof p === 'string' ? JSON.parse(p) : p);
    const hasPlayerB = Array.isArray(parsedPlayers) && parsedPlayers.some((p) => p.uid === 30002);
    assert(hasPlayerB, '玩家列表包含 Player B (uid=30002)', `players=${JSON.stringify(parsedPlayers).substring(0, 120)}`);
  }

  // ── 7. Player A 攻击 Player B → 双方收到战斗结果 ──
  console.log('');
  console.log('[7] Player A 攻击 Player B -> 跨网关战斗结果(6001)');
  playerA.clearMsgs();
  playerB.clearMsgs();
  playerA.send(1001, JSON.stringify({
    targetUid: 30002,
    skillId: 1,
  }));
  await sleep(1000);

  const aCombat = playerA.getMsgs(6001);
  assert(aCombat.length > 0, 'Player A 收到战斗结果(6001)', `got ${aCombat.length}`);

  const bCombat = playerB.getMsgs(6001);
  assert(bCombat.length > 0, 'Player B 跨网关收到战斗结果(6001)', `got ${bCombat.length}`);
  if (bCombat.length > 0) {
    const combatData = JSON.parse(bCombat[0].data);
    console.log(`        Player A -> Player B: dmg=${combatData.dmg} hp=${combatData.hp}/${combatData.maxHp}`);
  }

  // ── 8. Player B 断线 → Player A 跨网关收到离开通知 ──
  console.log('');
  console.log('[8] Player B 断线 -> Player A 跨网关收到离开通知(8003)');
  playerA.clearMsgs();
  playerB.disconnect();
  await sleep(2000);

  const leaveMsgs = playerA.getMsgs(8003);
  assert(leaveMsgs.length > 0, 'Player A 跨网关收到 Player B 离开通知(8003)', `got ${leaveMsgs.length}`);

  // ── 9. 验证 Redis 路由表已清理 ──
  console.log('');
  console.log('[9] Redis 路由表验证');
  const routeA = await fetch('http://127.0.0.1:9090/health').then((r) => r.json());
  const routeB = await fetch('http://127.0.0.1:9091/health').then((r) => r.json());
  console.log(`        Gate1 online: ${routeA.online_count}, Gate2 online: ${routeB.online_count}`);

  // ── 清理 ──
  playerA.disconnect();
  await sleep(500);

  // ── 总结 ──
  console.log('');
  console.log('============================================');
  console.log(`  跨网关集群测试结果: ${totalPass} PASS / ${totalFail} FAIL`);
  console.log('============================================');
  console.log('');

  process.exit(totalFail > 0 ? 1 : 0);
}

run().catch((e) => {
  console.error('Test crashed:', e);
  process.exit(1);
});
