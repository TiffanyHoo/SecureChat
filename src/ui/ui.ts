// 共享的 Tailwind 类名常量（设计系统原语），供各组件复用
export const buttonBase =
  'inline-flex cursor-pointer items-center justify-center gap-1 whitespace-nowrap rounded-lg text-sm font-medium shadow-sm transition focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus disabled:pointer-events-none disabled:opacity-50 active:scale-[0.98]';

export const buttonPrimary = `${buttonBase} bg-gradient-to-b from-accent to-accent-strong px-4 py-2 text-white hover:from-accent-strong hover:to-accent-strong`;

export const buttonSecondary = `${buttonBase} border border-line bg-surface px-3.5 py-2 text-ink hover:bg-surface-muted`;

export const buttonOver = `${buttonBase} min-w-24 bg-gradient-to-b from-over to-over-strong px-4 py-2 font-semibold text-white hover:from-over-strong hover:to-over-strong`;

export const buttonText = `${buttonBase} px-2 py-1 text-accent shadow-none hover:bg-accent-soft`;

export const inputBase =
  'w-full min-w-0 cursor-text rounded-lg border border-line-strong bg-surface px-2.5 py-2 text-sm text-ink transition placeholder:text-ink-muted/70 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus read-only:border-line read-only:text-ink-muted disabled:cursor-not-allowed';
