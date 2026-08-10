import * as React from "react";

import { Button, type ButtonProps } from "@/components/ui/button";

type IconButtonSize = "sm" | "rail" | "default" | "touch";

export interface IconButtonProps extends Omit<ButtonProps, "children" | "size"> {
  label: string;
  size?: IconButtonSize;
  children: React.ReactNode;
}

const sizeMap: Record<IconButtonSize, ButtonProps["size"]> = {
  sm: "iconSm",
  rail: "iconRail",
  default: "icon",
  touch: "iconTouch",
};

const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ label, size = "default", title, children, ...props }, ref) => (
    <Button ref={ref} size={sizeMap[size]} aria-label={label} title={title ?? label} {...props}>
      {children}
    </Button>
  ),
);

IconButton.displayName = "IconButton";

export { IconButton };
