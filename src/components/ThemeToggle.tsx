import { Moon, Sun } from "lucide-react";
import { AnimatePresence, motion, useMotionValue, useSpring } from "motion/react";

export type DashboardTheme = "day" | "moon";

export function ThemeToggle({ theme, onToggle }: { theme: DashboardTheme; onToggle(): void }) {
  const targetX = useMotionValue(0);
  const targetY = useMotionValue(0);
  const x = useSpring(targetX, { stiffness: 420, damping: 28, mass: .55 });
  const y = useSpring(targetY, { stiffness: 420, damping: 28, mass: .55 });
  const Icon = theme === "day" ? Sun : Moon;

  return <motion.button
    type="button"
    className="theme-toggle"
    style={{ x, y }}
    whileTap={{ scale: .84, rotate: theme === "day" ? 12 : -12 }}
    transition={{ type: "spring", stiffness: 560, damping: 24, mass: .45 }}
    aria-label={theme === "day" ? "切换到黑月主题" : "切换到白日主题"}
    title={theme === "day" ? "黑月主题" : "白日主题"}
    onPointerMove={event => {
      const bounds = event.currentTarget.getBoundingClientRect();
      targetX.set((event.clientX - bounds.left - bounds.width / 2) * .22);
      targetY.set((event.clientY - bounds.top - bounds.height / 2) * .22);
    }}
    onPointerLeave={() => { targetX.set(0); targetY.set(0); }}
    onClick={onToggle}
  >
    <AnimatePresence mode="popLayout" initial={false}>
      <motion.span
        key={theme}
        initial={{ opacity: 0, scale: .35, rotate: theme === "day" ? -80 : 80 }}
        animate={{ opacity: 1, scale: 1, rotate: 0 }}
        exit={{ opacity: 0, scale: .35, rotate: theme === "day" ? 80 : -80 }}
        transition={{ type: "spring", stiffness: 480, damping: 24, mass: .55 }}
      ><Icon/></motion.span>
    </AnimatePresence>
  </motion.button>;
}
