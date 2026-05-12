function shuffle<T>(arr: T[]): T[] {
    const shuffled = [...arr];
    for (let i = shuffled.length - 1; i > 0; i -= 1) {
        const j = Math.floor(Math.random() * (i + 1));
        [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
    }
    return shuffled;
}

const RAW_PLACEHOLDERS = [
    "Find that PDF you downloaded last week...",
    "The Word doc you were editing yesterday...",
    "A spreadsheet with budget numbers...",
    "That slide deck from last quarter...",
    "A README file in one of your projects...",
    "The design file with the new logo...",
    "A function you wrote a few weeks ago...",
    "That config file you changed recently...",
    "The repo with the authentication code...",
    "A pull request you reviewed last sprint...",
    "Find a file with a TODO you left yourself...",
    "An article you had open in a tab...",
    "A link you copied but never opened...",
    "That docs page you kept coming back to...",
    "A GitHub issue you commented on...",
    "A screenshot of an error message...",
    "That wireframe image from the design review...",
    "A photo you used in a presentation...",
    "Screenshot of something you wanted to remember...",
    "Notes from your last team meeting...",
    "An idea you typed out somewhere...",
    "A draft you never finished writing...",
    "Something you copied into a notes app...",
    "An email with a login link or invite...",
    "That message with the project brief...",
    "A Slack message you meant to follow up on...",
    "An email thread about the deadline...",
    "Something from a meeting two weeks ago...",
    "A file you opened right before a call...",
    "That thing you worked on over the weekend...",
];

export const PLACEHOLDERS = shuffle(RAW_PLACEHOLDERS);
