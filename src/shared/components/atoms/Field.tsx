import { forwardRef, type InputHTMLAttributes, type ReactNode } from "react";

type FieldVariant = "default" | "display";

interface FieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
    /** Left-aligned icon inside the input. */
    icon?: ReactNode;
    variant?: FieldVariant;
    error?: boolean;
}

/**
 * Text input atom.
 * - `default` — bg-2 surface, hairline border, amber focus halation. Used in forms.
 * - `display` — Cormorant 22px italic, no surface, hairline underline. Used in
 *   the command palette and immersive search heading.
 */
export const Field = forwardRef<HTMLInputElement, FieldProps>(function Field(
    { icon, variant = "default", error, className, ...rest },
    ref
) {
    const cls = [
        "fndr-field",
        variant === "display" ? "fndr-field--display" : "",
        icon ? "fndr-field--icon" : "",
        error ? "fndr-field--error" : "",
        className ?? "",
    ]
        .filter(Boolean)
        .join(" ");

    return (
        <div className="fndr-field-wrap">
            {icon && <span className="fndr-field-icon">{icon}</span>}
            <input ref={ref} className={cls} {...rest} />
        </div>
    );
});

export default Field;
