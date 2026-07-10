# 连接与握手场景
# 文档 4.1 节 BDD 行为用例

Feature: TCP连接与握手鉴权
  作为游戏客户端
  我需要通过TCP连接网关并完成握手
  以便建立安全的游戏通信通道

  # 正常连接
  Scenario: 正常TCP连接建立并进入握手阶段
    Given 客户端发起TCP连接到网关
    When 网关接受连接
    Then 连接应成功建立
    And 连接应进入握手阶段
    And 系统应分配连接资源

  # 黑名单拒绝
  Scenario: 黑名单IP直接拒绝连接
    Given IP "1.2.3.4" 已在黑名单中
    When 客户端从IP "1.2.3.4" 发起TCP连接
    Then 网关应直接拒绝连接
    And 不应分配任何资源
    And 应记录安全审计日志

  # 非法Token
  Scenario: 非法Token拒绝握手并断开连接
    Given 客户端建立TCP连接
    When 客户端发送握手包携带非法Token
    Then 网关应拒绝握手
    And 网关应断开连接
    And 应记录安全审计日志

  # 过期Token
  Scenario: 过期Token拒绝握手并断开连接
    Given 客户端建立TCP连接
    When 客户端发送握手包携带已过期的Token
    Then 网关应拒绝握手
    And 网关应断开连接

  # 版本不匹配
  Scenario: 客户端版本不匹配拒绝接入
    Given 客户端建立TCP连接
    When 客户端发送握手包携带版本号 "99"
    And 网关期望版本号为 "1"
    Then 网关应拒绝接入
    And 返回版本不匹配错误

  # 高频连接限流
  Scenario: 短时间高频重复连接触发临时限流
    Given 客户端从IP "5.6.7.8" 在5秒内发起20次连接
    When 客户端再次从IP "5.6.7.8" 发起连接
    Then 网关应触发连接限流
    And 应拒绝新连接
    And 应记录限流事件
