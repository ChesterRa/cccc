// useSwipeNavigation - 触摸手势导航 hook
import { useRef, useCallback } from "react";

interface UseSwipeNavigationOptions {
  tabs: string[];
  activeTab: string;
  onTabChange: (tab: string) => void;
  /** 最小水平滑动距离 (px) */
  minSwipeDistance?: number;
  /** 水平/垂直比例阈值 */
  swipeRatio?: number;
}

export function useSwipeNavigation({
  tabs,
  activeTab,
  onTabChange,
  minSwipeDistance = 50,
  swipeRatio = 1.5,
}: UseSwipeNavigationOptions) {
  const touchStartX = useRef<number>(0);
  const touchStartY = useRef<number>(0);

  const handleTouchStart = useCallback((e: React.TouchEvent) => {
    touchStartX.current = e.touches[0].clientX;
    touchStartY.current = e.touches[0].clientY;
  }, []);

  const handleTouchEnd = useCallback(
    (e: React.TouchEvent) => {
      const touchEndX = e.changedTouches[0].clientX;
      const touchEndY = e.changedTouches[0].clientY;
      const deltaX = touchEndX - touchStartX.current;
      const deltaY = touchEndY - touchStartY.current;

      // 判断是否为有效的水平滑动
      if (Math.abs(deltaX) > minSwipeDistance && Math.abs(deltaX) > Math.abs(deltaY) * swipeRatio) {
        const currentIndex = tabs.indexOf(activeTab);
        if (deltaX > 0 && currentIndex > 0) {
          // 向右滑动 -> 上一个 tab
          onTabChange(tabs[currentIndex - 1]);
        } else if (deltaX < 0 && currentIndex < tabs.length - 1) {
          // 向左滑动 -> 下一个 tab
          onTabChange(tabs[currentIndex + 1]);
        }
      }
    },
    [tabs, activeTab, onTabChange, minSwipeDistance, swipeRatio]
  );

  return {
    handleTouchStart,
    handleTouchEnd,
  };
}
