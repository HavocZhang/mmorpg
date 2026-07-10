# 集群跨网关行为
# 文档 4.6 节 BDD 行为用例

Feature: 集群跨网关协作
  作为网关集群
  我们需要协同工作
  以便实现百万在线无缝体验

  # 节点注册
  Scenario: 网关启动自动注册Redis服务发现
    Given 网关节点 "gate-01" 启动
    When 网关完成初始化
    Then 应向Redis注册节点信息
    And 节点信息应包含node_id、node_name、地址
    And 节点应加入集群节点集合

  # 心跳上报
  Scenario: 3秒心跳上报
    Given 网关节点 "gate-01" 已注册
    When 网关运行中
    Then 应每3秒向Redis上报心跳
    And 心跳应包含在线人数等状态

  # 宕机摘除
  Scenario: 网关宕机10秒内自动从集群摘除
    Given 网关节点 "gate-02" 在集群中
    When "gate-02" 停止心跳上报
    Then 10秒后 "gate-02" 应被从集群节点列表中摘除
    And 新连接不应路由到 "gate-02"

  # 本地直接下发
  Scenario: 同网关玩家消息本地直接下发零中间件
    Given 玩家 "10001" 和玩家 "10002" 都在网关 "gate-01"
    When "gate-01" 收到给 "10002" 的消息
    Then 消息应通过本地会话直接下发
    And 不应经过Redis PubSub

  # 跨网关投递
  Scenario: 跨网关玩家消息通过Redis PubSub精准投递不丢不重
    Given 玩家 "10001" 在网关 "gate-01"
    And 玩家 "10003" 在网关 "gate-02"
    When "gate-01" 收到给 "10003" 的消息
    Then "gate-01" 应通过Redis PubSub发布消息
    And "gate-02" 应订阅到该消息
    And "gate-02" 应将消息精准下发给 "10003"
    And 消息不应丢失
    And 消息不应重复投递

  # 重连路由更新
  Scenario: 玩家重连自动更新全局gate路由映射
    Given 玩家 "10001" 原在网关 "gate-01"
    When 玩家 "10001" 断线后重连到网关 "gate-02"
    Then Redis中 "10001" 的路由应更新为 "gate-02"
    And 后续跨网关消息应路由到 "gate-02"
