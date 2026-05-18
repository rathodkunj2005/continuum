import { motion } from "framer-motion";
import "./SectionTransitionBridge.css";

interface BridgeCard {
    id: string;
    title?: string;
}

interface SectionTransitionBridgeProps {
    /** Cards to fan out during the transition. 3-5 recommended. */
    cards?: BridgeCard[];
    /** Which section this bridge is attached to (used for layoutId). */
    sectionId: string;
}

const FAN_ANGLES = [-12, -5, 0, 5, 12];
const FAN_Y = [-6, -3, 0, -3, -6];

/**
 * Fan-of-cards morph transition shown between adjacent immersive sections.
 * Cards use Framer Motion `layoutId` to morph from/to real memory cards
 * elsewhere in the experience.
 */
export function SectionTransitionBridge({
    cards = [],
    sectionId,
}: SectionTransitionBridgeProps) {
    if (cards.length === 0) return null;

    const visible = cards.slice(0, 5);

    return (
        <div className="fndr-transition-bridge" aria-hidden>
            <div className="fndr-transition-fan">
                {visible.map((card, i) => {
                    const angle = FAN_ANGLES[i % FAN_ANGLES.length] ?? 0;
                    const y = FAN_Y[i % FAN_Y.length] ?? 0;
                    return (
                        <motion.div
                            key={card.id}
                            layoutId={`memory-card-${sectionId}-${card.id}`}
                            className="fndr-transition-card"
                            style={{
                                rotate: angle,
                                y,
                                zIndex: visible.length - i,
                            }}
                            initial={{ opacity: 0, scale: 0.9 }}
                            whileInView={{ opacity: 1, scale: 1 }}
                            viewport={{ once: true, amount: 0.6 }}
                            transition={{ duration: 0.45, delay: i * 0.06 }}
                        >
                            {card.title && (
                                <span className="fndr-transition-card-title">
                                    {card.title}
                                </span>
                            )}
                        </motion.div>
                    );
                })}
            </div>
        </div>
    );
}

export default SectionTransitionBridge;
