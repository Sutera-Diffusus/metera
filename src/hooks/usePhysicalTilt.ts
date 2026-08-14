import { useMotionValue, useSpring } from "motion/react";

export function usePhysicalTilt(intensity = 1) {
  const targetX = useMotionValue(0);
  const targetY = useMotionValue(0);
  const rotateX = useSpring(targetX, { stiffness: 260, damping: 28, mass: .8 });
  const rotateY = useSpring(targetY, { stiffness: 260, damping: 28, mass: .8 });

  return {
    style: { rotateX, rotateY, transformPerspective: 1100 },
    onPointerMove(event: React.PointerEvent<HTMLElement>) {
      const bounds = event.currentTarget.getBoundingClientRect();
      const horizontal = (event.clientX - bounds.left) / bounds.width - .5;
      const vertical = (event.clientY - bounds.top) / bounds.height - .5;
      targetX.set(vertical * -3.2 * intensity);
      targetY.set(horizontal * 3.8 * intensity);
    },
    onPointerLeave() {
      targetX.set(0);
      targetY.set(0);
    },
  };
}
