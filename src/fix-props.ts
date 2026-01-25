import { Project, SyntaxKind } from "ts-morph"

export type TFixIn = {
    root: string
    tsCfg: string
    filePath: string
}

export function fixPropsFile(input: TFixIn): string {
    const project = new Project({
        tsConfigFilePath: input.tsCfg
    })

    const sourceFile = project.addSourceFileAtPath(input.filePath)

    // Get all type aliases and interfaces
    const typeAliases = sourceFile.getTypeAliases()
    const interfaces = sourceFile.getInterfaces()

    // Filter to local (non-exported) declarations
    const localTypes = typeAliases.filter(t => !t.isExported())
    const localInterfaces = interfaces.filter(i => !i.isExported())

    const totalLocalDecls = localTypes.length + localInterfaces.length

    // Only proceed if there's exactly one local type/interface
    if (totalLocalDecls !== 1) {
        return sourceFile.getFullText()
    }

    if (localTypes.length === 1) {
        const typeAlias = localTypes[0]
        const oldName = typeAlias.getName()

        if (oldName !== "Props") {
            // Rename all references
            typeAlias.rename("Props")
        }
    } else if (localInterfaces.length === 1) {
        const iface = localInterfaces[0]
        const oldName = iface.getName()

        // Get the interface body text
        const members = iface.getMembers()
        const bodyText = members.map(m => m.getText()).join("\n")

        // Build new type alias text
        const newTypeText = `type Props = {\n  ${bodyText}\n};`

        // Replace interface with type alias (preserves surrounding whitespace)
        iface.replaceWithText(newTypeText)

        // Now rename old references to Props (if name changed)
        if (oldName !== "Props") {
            sourceFile.forEachDescendant(node => {
                if (node.getKind() === SyntaxKind.Identifier && node.getText() === oldName) {
                    node.replaceWithText("Props")
                }
            })
        }
    }

    return sourceFile.getFullText()
}
