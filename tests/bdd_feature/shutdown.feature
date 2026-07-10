# 容灾与优雅启停行为
# 文档 4.7 节 BDD 行为用例

Feature: 容灾与优雅启停
  作为网关
  我需要支持优雅启停和容灾恢复
  以便实现无宕机运维

  # 停止接受新连接
  Scenario: 收到停机信号不再接受新连接
    Given 网关正常运行中
    When 收到SIGTERM信号
    Then 网关应停止接受新TCP连接
    And TCP监听器应关闭
    And 应记录停机开始日志

  # 存量连接优雅下线
  Scenario: 存量连接逐个优雅下线通知逻辑服离线
    Given 网关有100个在线会话
    And 收到停机信号
    When 执行优雅停机
    Then 应逐个下线100个会话
    And 每个会话下线时应通知逻辑服
    And 应发送玩家离线消息
    And 应释放会话资源

  # 进程崩溃恢复
  Scenario: 进程崩溃不影响全局数据
    Given 网关节点 "gate-01" 运行中
    When 进程异常崩溃
    Then Redis中的会话缓存数据应不受影响
    And 其他网关节点应正常工作
    And 10秒后 "gate-01" 应从集群摘除
    And 玩家重连应分配到其他健康网关

  # 自动重连
  Scenario: 客户端断线自动重连自动分配健康网关
    Given 玩家 "10001" 原连接在网关 "gate-01"
    When "gate-01" 宕机
    And 玩家 "10001" 发起重连
    Then SLB应分配到其他健康网关 "gate-02"
    And "gate-02" 应接受连接
    And 路由映射应更新为 "gate-02"
    And 玩家应恢复游戏体验无感知

  # 启动就绪
  Scenario: 网关启动完成并就绪接受连接
    Given 网关进程启动
    When 配置加载完成
    And 日志系统初始化完成
    And 会话管理器初始化完成
    And 安全模块初始化完成
    And 集群注册完成
    And gRPC连接池建立完成
    And TCP监听器绑定成功
    Then 网关应进入就绪状态
    And 应开始接受客户端连接
    And 应开始心跳上报
