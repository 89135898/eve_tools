import type { Resource } from "i18next";

export const resources = {
  "zh-CN": {
    translation: {
      app: {
        title: "EVE 交易助手",
        subtitle: "吉他 4-4 空间站交易驾驶舱"
      },
      actions: {
        refresh: "刷新",
        refreshing: "刷新中",
        lookup: "查询"
      },
      language: {
        label: "语言",
        zhCN: "中文",
        enUS: "English"
      },
      statusCards: {
        backendApi: "后端 API",
        publicMarketSync: "公共市场同步",
        orderSync: "订单同步",
        dataSource: "数据来源"
      },
      lookup: {
        title: "市场价格查询",
        itemQuery: "物品查询",
        item: "物品",
        bestBid: "最高买价",
        bestAsk: "最低卖价",
        spread: "价差",
        dailyVolume: "日成交量",
        dataQuality: "数据质量"
      },
      selection: {
        title: "选品发现",
        count: "{{count}} 个候选",
        hub: "交易点",
        allHubs: "全部交易点",
        item: "物品",
        entry: "入场价",
        exit: "出场价",
        net: "净收益",
        attention: "关注度",
        reasons: "原因"
      },
      orders: {
        title: "订单监控",
        count: "{{count}} 个订单",
        item: "物品",
        side: "方向",
        current: "当前价",
        leader: "领先价",
        recommended: "建议价",
        action: "建议操作",
        urgency: "紧急度",
        reasons: "原因"
      },
      codes: {
        backendStatus: {
          ready: "就绪",
          degraded: "降级",
          offline: "离线",
          "not-configured": "未配置",
          unknown: "未知"
        },
        backendProbe: {
          ok: "正常",
          error: "错误",
          "not-configured": "未配置",
          unknown: "未知"
        },
        syncStatus: {
          "fixture-ready": "测试数据就绪",
          "fixture-fallback": "测试数据回退",
          "live-ready": "实时 ESI 就绪",
          "not-authorized": "未授权",
          unknown: "未知"
        },
        dataSource: {
          fixture: "测试数据",
          live: "实时 ESI",
          unknown: "未知"
        },
        trend: {
          up: "上涨",
          down: "下跌",
          stable: "稳定",
          unknown: "未知"
        },
        dataQuality: {
          fresh: "新鲜",
          stale: "过期",
          sparse: "稀疏",
          missing: "缺失"
        },
        side: {
          buy: "买单",
          sell: "卖单"
        },
        action: {
          raise: "上调",
          lower: "下调",
          hold: "保持"
        },
        reason: {
          healthy_spread: "价差健康",
          high_daily_volume: "日成交量高",
          deep_top_book: "盘口深度充足",
          acceptable_spread: "价差可接受",
          moderate_velocity: "成交速度适中",
          sparse_market_data: "市场数据稀疏",
          missing_market_side: "缺少单侧盘口",
          stale_market_data: "市场数据过期",
          negative_net_profit: "预估净收益为负",
          undercut_detected: "检测到被压价",
          high_velocity_item: "高流动性物品",
          overbid_detected: "检测到被超价"
        }
      }
    }
  },
  "en-US": {
    translation: {
      app: {
        title: "EVE Trader Assistant",
        subtitle: "Jita 4-4 station trading cockpit"
      },
      actions: {
        refresh: "Refresh",
        refreshing: "Refreshing",
        lookup: "Lookup"
      },
      language: {
        label: "Language",
        zhCN: "中文",
        enUS: "English"
      },
      statusCards: {
        backendApi: "Backend API",
        publicMarketSync: "Public market sync",
        orderSync: "Order sync",
        dataSource: "Data source"
      },
      lookup: {
        title: "Market Price Lookup",
        itemQuery: "Item query",
        item: "Item",
        bestBid: "Best bid",
        bestAsk: "Best ask",
        spread: "Spread",
        dailyVolume: "Daily volume",
        dataQuality: "Data quality"
      },
      selection: {
        title: "Selection Discovery",
        count: "{{count}} candidates",
        hub: "Hub",
        allHubs: "All hubs",
        item: "Item",
        entry: "Entry",
        exit: "Exit",
        net: "Net",
        attention: "Attention",
        reasons: "Reasons"
      },
      orders: {
        title: "Order Monitor",
        count: "{{count}} orders",
        item: "Item",
        side: "Side",
        current: "Current",
        leader: "Leader",
        recommended: "Recommended",
        action: "Action",
        urgency: "Urgency",
        reasons: "Reasons"
      },
      codes: {
        backendStatus: {
          ready: "Ready",
          degraded: "Degraded",
          offline: "Offline",
          "not-configured": "Not configured",
          unknown: "Unknown"
        },
        backendProbe: {
          ok: "OK",
          error: "Error",
          "not-configured": "Not configured",
          unknown: "Unknown"
        },
        syncStatus: {
          "fixture-ready": "Fixture ready",
          "fixture-fallback": "Fixture fallback",
          "live-ready": "Live ESI ready",
          "not-authorized": "Not authorized",
          unknown: "Unknown"
        },
        dataSource: {
          fixture: "Fixture",
          live: "Live ESI",
          unknown: "Unknown"
        },
        trend: {
          up: "Up",
          down: "Down",
          stable: "Stable",
          unknown: "Unknown"
        },
        dataQuality: {
          fresh: "Fresh",
          stale: "Stale",
          sparse: "Sparse",
          missing: "Missing"
        },
        side: {
          buy: "Buy",
          sell: "Sell"
        },
        action: {
          raise: "Raise",
          lower: "Lower",
          hold: "Hold"
        },
        reason: {
          healthy_spread: "Healthy spread",
          high_daily_volume: "High daily volume",
          deep_top_book: "Deep top book",
          acceptable_spread: "Acceptable spread",
          moderate_velocity: "Moderate velocity",
          sparse_market_data: "Sparse market data",
          missing_market_side: "Missing one side of book",
          stale_market_data: "Stale market data",
          negative_net_profit: "Estimated net profit is negative",
          undercut_detected: "Undercut detected",
          high_velocity_item: "High velocity item",
          overbid_detected: "Overbid detected"
        }
      }
    }
  }
} satisfies Resource;
