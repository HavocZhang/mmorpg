// ====== MMORPG 物品子系统 ======
// 服务端 ItemDef 同步副本 / 装备槽 / 药水效果 / 掉落规则

const ITEMS = {
  // 武器 (weapon) — 装备到武器槽
  1:  { name:'铁剑',     icon:'🗡',  type:'weapon',    atk:15, def:0,  hp:0, mp:0, sell:100,  desc:'普通铁制长剑，+15攻击' },
  2:  { name:'钢剑',     icon:'⚔',  type:'weapon',    atk:30, def:0,  hp:0, mp:0, sell:300,  desc:'精炼钢剑，+30攻击' },

  // 护甲 (armor) — 装备到护甲槽
  3:  { name:'皮甲',     icon:'🛡',  type:'armor',     atk:0,  def:10, hp:0, mp:0, sell:150,  desc:'轻便皮甲，+10防御' },
  4:  { name:'铁甲',     icon:'🛡',  type:'armor',     atk:0,  def:25, hp:0, mp:0, sell:400,  desc:'厚重铁甲，+25防御' },

  // 饰品 (accessory) — 装备到饰品槽
  5:  { name:'力量戒指', icon:'💍',  type:'accessory', atk:10, def:5,  hp:0, mp:0, sell:200,  desc:'散发力量的戒指，+10攻击 +5防御' },

  // 药水 (potion) — 使用后消耗
  6:  { name:'生命药水', icon:'🧪',  type:'potion',    atk:0,  def:0,  hp:50,mp:0, sell:50,   desc:'恢复 50 生命值' },
  7:  { name:'法力药水', icon:'💧',  type:'potion',    atk:0,  def:0,  hp:0, mp:30,sell:50,   desc:'恢复 30 法力值' },
  8:  { name:'全恢复药水',icon:'💎',  type:'potion',    atk:0,  def:0,  hp:100,mp:50,sell:150, desc:'恢复 100 生命和 50 法力' },

  // 材料 (material) — 任务/合成用
  9:  { name:'史莱姆凝胶',icon:'🟢', type:'material',  atk:0,  def:0,  hp:0, mp:0, sell:10,   desc:'史莱姆身上掉落的凝胶' },
  10: { name:'哥布林耳朵',icon:'👂', type:'material',  atk:0,  def:0,  hp:0, mp:0, sell:15,   desc:'哥布林的战利品' },
};

// 装备槽定义
const SLOTS = {
  weapon:    { name:'武器', icon:'⚔', types:['weapon'] },
  armor:     { name:'护甲', icon:'🛡', types:['armor'] },
  accessory: { name:'饰品', icon:'💍', types:['accessory'] },
};

// 怪物掉落表(与 logic_server 同步)
const DROP_TABLE = {
  1: { name:'史莱姆',     guaranteed:[9],           chance:[{id:6, rate:0.33}] },
  2: { name:'哥布林',     guaranteed:[10],          chance:[{id:7, rate:0.50}] },
  3: { name:'骷髅战士',   guaranteed:[1],           chance:[{id:6, rate:0.50}] },
  4: { name:'暗影法师',   guaranteed:[2,8],         chance:[] },
  5: { name:'岩石巨人',   guaranteed:[4,5,8],       chance:[] },
};

function getItem(id) { return ITEMS[id] || { name:'未知物品', icon:'📦', type:'unknown', atk:0, def:0, hp:0, mp:0, sell:0, desc:'未知物品' }; }

// 合并背包物品(服务端格式 [{itemId,count}] → 客户端合并格式)
function mergeInv(serverItems) {
  const map = {};
  (serverItems||[]).forEach(si => {
    const id = si.itemId || si[0];
    if (!id) return;
    map[id] = (map[id]||0) + (si.count||si[1]||1);
  });
  return Object.entries(map).map(([id,count]) => ({ id: parseInt(id), count, def: getItem(parseInt(id)) }));
}

// 计算装备总加成
function calcEquipBonuses(equip) {
  let atk=0, def=0;
  if (!equip) return {atk,def};
  ['weapon','armor','accessory'].forEach(slot => {
    const item = equip[slot];
    if (item && item.itemId) {
      const def = getItem(item.itemId);
      atk += def.atk;
      def += def.def;
    }
  });
  return {atk, def};
}

if (typeof module !== 'undefined') module.exports = { ITEMS, SLOTS, DROP_TABLE, getItem, mergeInv, calcEquipBonuses };
