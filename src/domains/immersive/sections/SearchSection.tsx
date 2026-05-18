import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { StickyScene } from "@/domains/immersive/components/StickyScene";
import { useSearch } from "@/shared/hooks/useSearch";
import { useSetWallpaperPage } from "@/shared/hooks/useImmersiveWallpaper";
import { MemoryCard } from "@/domains/memory-vault/MemoryCard";
import "./SearchSection.css";

const SECTION_ID = "search";

/**
 * Slice 5 — Semantic search scene.
 * Cormorant display input, query intent chips, staggered retrieval results.
 */
export function SearchSection() {
    useSetWallpaperPage("search", SECTION_ID);

    const [query, setQuery] = useState("");
    const { results, isLoading } = useSearch(query, null, null);

    // Derive intent chips from query words (first 4, capitalized)
    const chips = query
        .trim()
        .split(/\s+/)
        .filter(Boolean)
        .slice(0, 4)
        .map((w) => w.charAt(0).toUpperCase() + w.slice(1).toLowerCase());

    return (
        <StickyScene sectionId={SECTION_ID} scrollBudget={800}>
            <div className="fndr-search-scene">
                <div className="fndr-search-scene-inner">
                    <motion.p
                        className="fndr-search-label"
                        initial={{ opacity: 0, y: 8 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.5 }}
                    >
                        Semantic retrieval
                    </motion.p>

                    <motion.h2
                        className="fndr-search-heading"
                        initial={{ opacity: 0, y: 14 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.6, delay: 0.08 }}
                    >
                        find a memory
                    </motion.h2>

                    <motion.div
                        className="fndr-search-field-row"
                        initial={{ opacity: 0, y: 10 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true }}
                        transition={{ duration: 0.5, delay: 0.16 }}
                    >
                        <input
                            className="fndr-search-field"
                            value={query}
                            onChange={(e) => setQuery(e.target.value)}
                            placeholder="search your memories…"
                            spellCheck={false}
                        />
                        {isLoading && <div className="fndr-search-spinner" />}
                    </motion.div>

                    {/* Intent chips */}
                    <AnimatePresence>
                        {chips.length > 0 && (
                            <motion.div
                                className="fndr-search-chips"
                                initial={{ opacity: 0, y: 6 }}
                                animate={{ opacity: 1, y: 0 }}
                                exit={{ opacity: 0 }}
                                transition={{ duration: 0.25 }}
                            >
                                {chips.map((chip, i) => (
                                    <motion.span
                                        key={chip}
                                        className="fndr-search-chip"
                                        initial={{ opacity: 0, scale: 0.85 }}
                                        animate={{ opacity: 1, scale: 1 }}
                                        transition={{ delay: i * 0.06 }}
                                    >
                                        {chip}
                                    </motion.span>
                                ))}
                            </motion.div>
                        )}
                    </AnimatePresence>

                    {/* Results */}
                    <div className="fndr-search-results">
                        <AnimatePresence mode="popLayout">
                            {results.slice(0, 5).map((card, i) => (
                                <motion.div
                                    key={card.id}
                                    className="fndr-search-result-row"
                                    initial={{ opacity: 0, y: 12 }}
                                    animate={{ opacity: 1, y: 0 }}
                                    exit={{ opacity: 0, y: -8 }}
                                    transition={{ delay: i * 0.06, duration: 0.3 }}
                                >
                                    <MemoryCard card={card} variant="compact" />
                                </motion.div>
                            ))}
                        </AnimatePresence>

                        {!query && !isLoading && (
                            <motion.p
                                className="fndr-search-empty"
                                initial={{ opacity: 0 }}
                                animate={{ opacity: 1 }}
                                transition={{ delay: 0.4 }}
                            >
                                type to retrieve memories from your machine
                            </motion.p>
                        )}
                    </div>
                </div>
            </div>
        </StickyScene>
    );
}

export default SearchSection;
