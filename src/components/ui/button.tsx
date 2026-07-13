import { cva, type VariantProps } from "class-variance-authority";
import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { cn } from "../../lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 border text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#b3cc72] focus-visible:ring-offset-2 focus-visible:ring-offset-[#101110] disabled:pointer-events-none disabled:opacity-45",
  {
    variants: {
      variant: {
        primary:
          "border-[#c2dc79] bg-[#b3cc72] text-[#11140c] hover:border-[#d4eb94] hover:bg-[#c2dc79]",
        secondary:
          "border-white/12 bg-white/[0.04] text-[#f1f3ee] hover:border-white/22 hover:bg-white/[0.08]",
        quiet:
          "border-transparent bg-transparent text-[#aeb2aa] hover:bg-white/[0.06] hover:text-[#f1f3ee]",
        danger:
          "border-[#b95f4a]/50 bg-[#b95f4a]/10 text-[#f1afa0] hover:bg-[#b95f4a]/20",
      },
      size: {
        default: "h-10 px-4 rounded-md",
        compact: "h-8 px-3 rounded-md text-xs",
        icon: "size-9 rounded-md p-0",
      },
    },
    defaultVariants: {
      variant: "secondary",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends
    ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  children?: ReactNode;
}

const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, type = "button", ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  ),
);

Button.displayName = "Button";

export { Button, buttonVariants };
