import type { ButtonHTMLAttributes, ReactNode } from "react";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "alarm";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: ButtonVariant;
    /** Use mono caps styling (typewriter look). */
    mono?: boolean;
    icon?: ReactNode;
    children?: ReactNode;
}

export function Button({
    variant = "secondary",
    mono = false,
    icon,
    children,
    className,
    type = "button",
    ...rest
}: ButtonProps) {
    const cls = [
        "continuum-button",
        `continuum-button--${variant}`,
        mono ? "continuum-button--mono" : "",
        className ?? "",
    ]
        .filter(Boolean)
        .join(" ");

    return (
        <button type={type} className={cls} {...rest}>
            {icon}
            {children}
        </button>
    );
}

export default Button;
