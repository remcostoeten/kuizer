import { dim, bold, colorize, badge, rule, indent, write } from "../output/index.js";
import type { Severity } from "../types.js";

write(bold("Kuizer"));
write(rule());
write(colorize("All good!", "info" as Severity));
