# 会话 Session 行为
# 文档 4.2 节 BDD 行为用例

Feature: 会话Session管理
  作为网关
  我需要管理所有在线玩家的会话
  以便维护连接状态和消息路由

  # 唯一Session创建
  Scenario: 新连接成功创建唯一Session
    Given 客户端完成握手
    When 网关创建会话
    Then 会话应创建成功
    And session_id 应全局唯一
    And 会话状态应为 "Online"

  # 绑定player_uid
  Scenario: 登录成功绑定player_uid与会话映射
    Given 客户端完成握手且player_uid为 "10001"
    When 网关创建会话
    Then 会话应绑定player_uid "10001"
    And player_uid到session_id的映射应建立
    And 通过player_uid可查询到对应会话

  # 顶号机制
  Scenario: 同一账号新登录自动顶掉旧会话
    Given player_uid "10001" 已存在在线会话 session_id "A"
    When 同一player_uid "10001" 再次登录
    Then 旧会话 session_id "A" 应被下线
    And 新会话应创建成功
    And 旧会话的发送通道应关闭
    And 应记录顶号事件

  # 僵尸连接清理
  Scenario: 45秒无心跳无交互自动判定僵尸连接并清理
    Given 存在在线会话session_id "B"
    And 会话 "B" 最后活跃时间为44秒前
    When 心跳巡检执行
    Then 会话 "B" 不应被清理
    When 时间超过45秒
    And 心跳巡检执行
    Then 会话 "B" 应被判定为僵尸连接
    And 会话 "B" 应被清理
    And 相关资源应被释放

  # 资源完整释放
  Scenario: 会话关闭完整释放TCP资源和映射表
    Given 存在在线会话session_id "C" 绑定player_uid "10002"
    When 会话 "C" 被关闭
    Then session_map中应移除 session_id "C"
    And uid_map中应移除 player_uid "10002"
    And TCP文件描述符应释放
    And 在线连接计数应减一
