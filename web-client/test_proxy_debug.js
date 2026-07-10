/**
 * WebSocket 代理调试测试
 * 记录所有收到的事件，排查代理消息转发问题
 */

const WebSocket = require("ws");

const url = "ws://localhost:3000/ws";
const ws = new WebSocket(url);

let eventCount = 0;
let messageCount = 0;

ws.on("open", () => {
  console.log("[WS] Connected to proxy");
});

ws.on("message", (raw) => {
  const msg = JSON.parse(raw.toString());
  eventCount++;
  
  // 记录所有事件
  if (msg.event === "ready") {
    console.log(`[EVENT #${eventCount}] ready: config=${JSON.stringify(msg.config)}`);
    
    // 发送连接命令
    console.log("[ACTION] Sending connect command...");
    ws.send(JSON.stringify({
      action: "connect",
      id: "test-1",
      uid: 30001,
      token: "test_token_123",
      host: "127.0.0.1",
      port: 7888,
    }));
  } else if (msg.event === "connected") {
    console.log(`[EVENT #${eventCount}] connected: id=${msg.id} uid=${msg.uid}`);
  } else if (msg.event === "message") {
    messageCount++;
    console.log(`[EVENT #${eventCount}] MESSAGE: id=${msg.id} msgId=${msg.msgId} data=${msg.data}`);
  } else if (msg.event === "stats") {
    // 只打印第一条和最后一条 stats
    if (messageCount === 0) {
      console.log(`[EVENT #${eventCount}] stats: ${JSON.stringify(msg)}`);
    }
  } else if (msg.event === "error") {
    console.log(`[EVENT #${eventCount}] ERROR: id=${msg.id} message=${msg.message}`);
  } else if (msg.event === "disconnected") {
    console.log(`[EVENT #${eventCount}] disconnected: id=${msg.id} reason=${msg.reason}`);
  } else {
    console.log(`[EVENT #${eventCount}] ${msg.event}: ${JSON.stringify(msg).substring(0, 200)}`);
  }
});

ws.on("error", (err) => {
  console.log(`[WS ERROR] ${err.message}`);
});

ws.on("close", () => {
  console.log(`[WS] Closed. Total events: ${eventCount}, Messages: ${messageCount}`);
});

// 3秒后如果没有消息，发送一条聊天
setTimeout(() => {
  if (messageCount === 0) {
    console.log("\n[WARN] No messages received after 3s. Sending chat anyway...");
  }
  console.log("[ACTION] Sending chat message...");
  ws.send(JSON.stringify({
    action: "send",
    id: "test-1",
    msgId: 2001,
    data: JSON.stringify({ text: "Hello via proxy!" }),
  }));
}, 3000);

// 8秒后关闭
setTimeout(() => {
  console.log(`\n[DONE] Total events: ${eventCount}, Messages: ${messageCount}`);
  if (messageCount === 0) {
    console.log("[FAIL] No game messages received through proxy!");
  } else {
    console.log("[PASS] Messages received through proxy!");
  }
  ws.close();
  process.exit(0);
}, 8000);
