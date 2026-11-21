// 共享动效预设（轻盈灵动风格）：统一全应用的弹性手感，避免各组件自定参数漂移
import type { Transition } from 'motion/react';

/** 气泡/卡片/视图入场：一次可感知的小回弹 */
export const springEnter: Transition = {
  type: 'spring',
  stiffness: 380,
  damping: 20,
  mass: 1,
};

/** 小元素 pop（回应 chips、数字跳动）：更脆、更轻快的 spring */
export const springPop: Transition = {
  type: 'spring',
  stiffness: 700,
  damping: 18,
};

/** 表情单次跳动：放大后回落，pill 外框不参与缩放 */
export const emojiPulse: Transition = {
  duration: 0.34,
  ease: 'easeInOut',
};

export const emojiMaxScale = 2.5;

/** 回应 chip 取消：下滑淡出，短促干净，不回弹 */
export const chipExit: Transition = {
  duration: 0.15,
  ease: 'easeOut',
};

/** 已连接状态灯：柔和波纹扩散 */
export const statusRipple: Transition = {
  duration: 1.45,
  ease: 'easeOut',
  repeat: Infinity,
  repeatDelay: 0.4,
};

/** 状态呼吸脉冲（等待/OVER 已发出）：柔和循环 */
export const breathPulse: Transition = {
  duration: 1.4,
  ease: 'easeInOut',
  repeat: Infinity,
};
