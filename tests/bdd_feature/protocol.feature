# 协议编解码行为
# 文档 4.3 节 BDD 行为用例

Feature: 私有协议编解码
  作为网关
  我需要正确编解码游戏私有协议
  以便安全传输和防篡改

  # 正常编解码
  Scenario: 合法封包正常解码
    Given 使用正确的AES密钥
    When 客户端发送合法加密封包
    Then 网关应成功解码
    And 解码后的消息体应与原始数据一致

  # 粘包处理
  Scenario: 粘包自动拆分多包合并解析正确
    Given 客户端连续发送3个封包
    When 数据在TCP缓冲区中形成粘包
    Then 网关应自动拆分为3个独立包
    And 每个包的消息体应正确解析

  # 半包处理
  Scenario: 半包等待后续数据补全后解析
    Given 客户端发送一个封包
    When TCP先到达包头但包体不完整
    Then 网关应等待后续数据
    When 后续数据到达补全包体
    Then 网关应成功解析完整包

  # 超大包防护
  Scenario: 单包超过8KB直接断连防护OOM
    Given 客户端发送包体大小为8193字节
    When 网关解析包头发现包体超限
    Then 网关应直接断开连接
    And 应记录安全日志
    And 应更新解码错误指标

  # CRC校验失败
  Scenario: CRC校验失败直接断连记录安全日志
    Given 客户端发送封包
    When 包体的CRC32与包头中记录的不一致
    Then 网关应直接断开连接
    And 应记录CRC校验失败安全日志

  # AES解密失败
  Scenario: AES解密失败非法包头直接断连
    Given 客户端发送封包
    When 包体无法被AES-GCM正确解密
    Then 网关应直接断开连接
    And 应记录AES解密失败安全日志

  # 畸形包拦截
  Scenario: 空包畸形包恶意数据包拦截
    Given 客户端发送空包
    Then 网关应拦截并断开连接
    Given 客户端发送魔数错误的包
    Then 网关应拦截并断开连接
    Given 客户端发送随机畸形数据
    Then 网关应拦截并断开连接
