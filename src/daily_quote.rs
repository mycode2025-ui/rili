//! Offline daily quote bank.
//!
//! Complete opening and closing clauses are combined into natural, unattributed
//! sentences. Selection happens locally and does not require network access.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const OPENINGS: [&str; 48] = [
    "把复杂的事情拆小",
    "认真完成眼前的一步",
    "真正的进步来自持续行动",
    "保持好奇，也保持耐心",
    "给重要的事情留出时间",
    "先建立秩序，再追求速度",
    "清醒地选择自己的方向",
    "允许计划随着事实调整",
    "把注意力放回可以改变的部分",
    "长期积累胜过短暂冲刺",
    "每一次复盘都在缩短弯路",
    "做好小事，也是在完成大事",
    "在安静中整理自己的判断",
    "不急于证明，先认真完成",
    "让行动比焦虑更早发生",
    "今天的专注会成为明天的从容",
    "接受不完美的开始",
    "稳定的节奏比偶尔用力更可靠",
    "把目标写清楚，把步骤做扎实",
    "尊重时间，也尊重自己的感受",
    "面对变化时保留一点弹性",
    "从真实的问题出发",
    "在忙碌里守住最重要的一件事",
    "让每个选择都更接近想要的生活",
    "把今天过得具体一些",
    "先解决真正影响结果的问题",
    "在重复中寻找可以改进的细节",
    "把心力用在值得的地方",
    "给正在成长的自己多一点耐心",
    "用清晰的边界保护专注",
    "先迈出一步，再修正方向",
    "把经验沉淀成下一次的从容",
    "愿意倾听，也敢于做出判断",
    "让重要的事拥有完整的时间",
    "照顾好状态，再处理复杂的问题",
    "把困难看成需要拆解的信息",
    "在纷杂里重新找到主线",
    "保持谦逊，也相信自己的积累",
    "把想法落实成一个小小的行动",
    "为值得的目标保留韧性",
    "不被一时的结果定义",
    "在每次尝试后留下新的经验",
    "用行动回应心里的期待",
    "把普通的一天过得有分寸",
    "允许自己停下来重新校准",
    "从容来自一次次认真准备",
    "让选择忠于内心也尊重现实",
    "珍惜此刻仍然拥有的可能",
];

const ENDINGS: [&str; 24] = [
    "时间会让答案逐渐清晰",
    "微小的完成也值得被看见",
    "方向正确时慢一点也没关系",
    "持续本身就是一种力量",
    "你会比昨天更接近目标",
    "专注会替你过滤无关的噪声",
    "耐心能把困难变成可以处理的步骤",
    "好的结果往往藏在日常重复里",
    "行动会带来新的信息和选择",
    "清晰比仓促更接近效率",
    "给自己留一点思考和呼吸的空间",
    "长期主义终会显出它的价值",
    "完成比想象中的完美更重要",
    "节奏稳定之后，困难也会变轻",
    "认真生活的人自有自己的光",
    "今天仍然可以成为新的起点",
    "踏实会让远方慢慢靠近",
    "每一步都在塑造未来的自己",
    "慢下来也可能是更好的前进",
    "内心笃定时脚步自然轻盈",
    "答案常常出现在行动之后",
    "你已经在路上了",
    "平凡的坚持终会留下痕迹",
    "留白也能为下一步积蓄力量",
];

pub const QUOTE_COUNT: usize = OPENINGS.len() * ENDINGS.len();
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn quote_at(index: usize) -> String {
    let opening = OPENINGS[index % OPENINGS.len()];
    let ending = ENDINGS[(index / OPENINGS.len()) % ENDINGS.len()];
    format!("{opening}，{ending}。")
}

/// Returns a locally selected quote. Time, process id and a monotonic sequence
/// are mixed so consecutive application launches do not keep choosing the same
/// sentence even when they happen very close together.
pub fn random_quote() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut mixed = nanos ^ (u64::from(std::process::id()) << 32) ^ sequence;
    mixed ^= mixed >> 30;
    mixed = mixed.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed ^= mixed >> 27;
    mixed = mixed.wrapping_mul(0x94d0_49bb_1331_11eb);
    mixed ^= mixed >> 31;
    quote_at(mixed as usize % QUOTE_COUNT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn contains_hundreds_of_unique_offline_phrases() {
        let phrases: HashSet<_> = (0..QUOTE_COUNT).map(quote_at).collect();
        assert_eq!(phrases.len(), 1152);
    }

    #[test]
    fn random_quote_always_comes_from_the_offline_bank() {
        let quote = random_quote();
        assert!((0..QUOTE_COUNT).any(|index| quote_at(index) == quote));
    }
}
