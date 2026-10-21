const warmLogoConcepts = [
  {
    id: 'ember-eraser',
    number: '01',
    name: 'Ember Eraser',
    category: 'The amber eraser',
    image: '/logos/v6/ember-eraser.png',
    material: 'Amber glass · Ivory rubber',
    description:
      'An ivory eraser in a translucent orange sleeve, set diagonally against graphite. Two small crumbs connect the object to the act of clearing files.',
    idea: 'Clear what you choose.',
    colors: ['#fff0da', '#f0a56b', '#d97757', '#171717'],
  },
  {
    id: 'fold',
    number: '02',
    name: 'Fold',
    category: 'The folded K',
    image: '/logos/v6/fold.png',
    material: 'Copper glass · Satin ivory',
    description:
      'A broad copper ribbon folds into a K. Its ivory inner faces and illuminated edges stand against a dark tile.',
    idea: 'Give the name a recognizable shape.',
    colors: ['#ffd9a6', '#d97757', '#bf603d', '#171717'],
  },
  {
    id: 'clearspace',
    number: '03',
    name: 'Clearspace',
    category: 'The reclaimed tile',
    image: '/logos/v6/clearspace.png',
    material: 'Jade glass · Ivory ceramic · Copper',
    description:
      'Ivory and jade tiles form a disk usage map. A small copper tile lifts from its recess, exposing the space underneath.',
    idea: 'See the space you reclaim.',
    colors: ['#f4eee1', '#335f4f', '#d97757', '#171717'],
  },
] as const

const wildAlternatives = [
  {
    id: 'slipstream',
    number: '01',
    name: 'Slipstream',
    category: 'The flowing gesture',
    image: '/logos/wild/slipstream.png',
    material: 'Continuous movement · Folded gradients',
    description:
      'The eraser stretched into a flowing ribbon, with a broad beginning, an open center, and a tapering exit. Its shape carries the clearing gesture into an abstract mark.',
    idea: 'Follow the movement instead of the object.',
    colors: ['#f7efec', '#d0b8ef', '#ab64dd', '#38214e'],
  },
  {
    id: 'phase-shift',
    number: '02',
    name: 'Phase Shift',
    category: 'The broken geometry',
    image: '/logos/wild/phase-shift.png',
    material: 'Angled planes · Open cuts · Offset parts',
    description:
      'An angular mark broken into offset pieces. Zigzag gaps and a short trail of fragments turn the original eraser into a study of geometry and negative space.',
    idea: 'Take the shape apart and let the gaps define it.',
    colors: ['#f3eddf', '#c1a7e9', '#7d41db', '#372053'],
  },
] as const

const eraserVariants = [
  {
    id: 'soft-slate',
    number: '01',
    name: 'Soft Slate',
    category: 'The softer eraser',
    image: '/logos/eraser-variants/soft-slate.png',
    material: 'Soft forms · Lavender shading',
    description:
      'The diagonal eraser with softer shapes and broad highlights. A smooth lavender collar and a few simple crumbs carry the original idea with less surface detail.',
    idea: 'Rounded, gently shaded, and readable at a glance.',
    colors: ['#f4efff', '#c5acf0', '#9674dc', '#6545b6'],
  },
  {
    id: 'graphic-slate',
    number: '02',
    name: 'Graphic Slate',
    category: 'The graphic eraser',
    image: '/logos/eraser-variants/graphic-slate.png',
    material: 'Bold shapes · Selective gradients',
    description:
      'A two-tone eraser with a direct color division, simple edges, and a shallow shadow. The purple tile frames a more graphic version of the same mark.',
    idea: 'Fewer details, with the silhouette doing the work.',
    colors: ['#f3edde', '#b6a0ed', '#7756ce', '#322451'],
  },
] as const

const newestLogoConcepts = [
  {
    id: 'bristle',
    number: '01',
    name: 'Bristle',
    category: 'The cleaning tool',
    image: '/logos/v3/bristle.png',
    material: 'Orange enamel · Nickel · Ivory bristles',
    description:
      'A compact brush with an enamel handle and a fan of fine bristles. The sweeping silhouette connects directly to cleanup, with the detail of a carefully made desktop tool.',
    idea: 'A small tool for clearing the accumulated dust.',
    colors: ['#f0dfbe', '#cfcbc1', '#e06a36', '#7e3020'],
  },
  {
    id: 'ginkgo',
    number: '02',
    name: 'Ginkgo',
    category: 'The organic mark',
    image: '/logos/v3/ginkgo.png',
    material: 'Jade glass · Champagne brass',
    description:
      'A single fan-shaped leaf with radial veins and a notched edge. It gives Kuizer an organic emblem, using depth and material to bring a simple silhouette into focus.',
    idea: 'An identity drawn from natural forms.',
    colors: ['#cadac3', '#8ba88c', '#335f4f', '#bba677'],
  },
  {
    id: 'pocket',
    number: '03',
    name: 'Pocket',
    category: 'The file object',
    image: '/logos/v3/pocket.png',
    material: 'Amber glass · Pearl ceramic',
    description:
      'An open folder in layered amber glass, holding one ceramic file. The stepped tab gives it a familiar shape, while the overlapping surfaces reveal how it is constructed.',
    idea: 'A familiar home for the files you keep.',
    colors: ['#f5ead6', '#efb667', '#bb732c', '#64412b'],
  },
  {
    id: 'aperture',
    number: '04',
    name: 'Aperture',
    category: 'The open center',
    image: '/logos/v3/aperture.png',
    material: 'Cobalt aluminum · Silver edges',
    description:
      'Three curved blades surround an open center. Machined surfaces and overlapping edges turn the space between the pieces into the defining part of the mark.',
    idea: 'Give the empty space a shape of its own.',
    colors: ['#cbd5e4', '#6283d3', '#3153a5', '#1d2c58'],
  },
] as const

export const logoConcepts = [
  {
    id: 'marten',
    number: '01',
    name: 'Marten',
    category: 'The character',
    image: '/logos/v2/marten.png',
    material: 'Warm porcelain · Chestnut ceramic',
    description:
      'A curled woodland character with a compact silhouette. The face and sweeping tail give Kuizer a recognizable mascot, with the finish of a small ceramic object.',
    idea: 'A character you could recognize without the name.',
    colors: ['#fff0da', '#d58c46', '#8c4728', '#35291f'],
  },
  {
    id: 'cutaway',
    number: '02',
    name: 'Cutaway',
    category: 'The negative space',
    image: '/logos/v2/cutaway.png',
    material: 'Brushed titanium · Vermilion aluminum',
    description:
      'A solid block with a section removed. The gap carries the idea of reclaiming space, while the graphite shell and orange interior give the mark an industrial character.',
    idea: 'Make room by taking something away.',
    colors: ['#c9c6c0', '#ee7138', '#484a49', '#202323'],
  },
  {
    id: 'strata',
    number: '03',
    name: 'Strata',
    category: 'The ordered layers',
    image: '/logos/v2/strata.png',
    material: 'Ivory porcelain · Sage metal',
    description:
      'Three layers step down in length, turning a stack of accumulated files into a simple rhythm. A geometric direction with warm surfaces and a muted green palette.',
    idea: 'Less accumulation, one layer at a time.',
    colors: ['#eee9d9', '#a3b697', '#577663', '#253b31'],
  },
  {
    id: 'prompt',
    number: '04',
    name: 'Prompt',
    category: 'The developer object',
    image: '/logos/v2/prompt.png',
    material: 'Textured graphite · Chartreuse resin',
    description:
      'A sculpted workstation key with a terminal prompt inlaid into its face. Dark texture, metal edges, and one sharp color connect the identity to Kuizer’s command-line roots.',
    idea: 'A familiar symbol made into a physical object.',
    colors: ['#d2e772', '#a4b849', '#51544c', '#20221f'],
  },
] as const

export const previousLogoConcepts = [
  {
    id: 'prism',
    number: '01',
    name: 'Prism K',
    category: 'The monogram',
    image: '/logos/prism-k.png',
    material: 'Glacier glass · Petrol enamel',
    description:
      'A K built from interlocking glass. A direct connection to the name, with cool edges and enough depth to feel like a physical object.',
    idea: 'A precise tool with a distinct signature.',
    colors: ['#baf7fa', '#48bec9', '#146a7b', '#10323e'],
  },
  {
    id: 'sweep',
    number: '02',
    name: 'Sweep',
    category: 'The gesture',
    image: '/logos/sweep.png',
    material: 'Satin glass · Pearl ceramic',
    description:
      'A warm ribbon sweeps loose blocks into order. The open circle suggests space being reclaimed, while the ceramic base keeps the mark grounded.',
    idea: 'Less clutter. A little room to breathe.',
    colors: ['#fff0d6', '#ffb36d', '#f97844', '#c74135'],
  },
  {
    id: 'slate',
    number: '03',
    name: 'Clean Slate',
    category: 'The object',
    image: '/logos/clean-slate.png',
    material: 'Frosted violet · Porcelain · Chrome',
    description:
      'A familiar eraser, reimagined in lavender glass and porcelain. A tangible metaphor for clearing away generated files and starting fresh.',
    idea: 'A useful little object for your desktop.',
    colors: ['#eee6ff', '#c0a0f2', '#8861cd', '#44326b'],
  },
] as const

export const logoRounds = [
  { id: '06', name: 'New concepts', concepts: warmLogoConcepts },
  { id: '05', name: 'Wild alternatives', concepts: wildAlternatives },
  { id: '04', name: 'Eraser variants', concepts: eraserVariants },
  { id: '03', name: 'Round 03', concepts: newestLogoConcepts },
  { id: '02', name: 'Round 02', concepts: logoConcepts },
  { id: '01', name: 'Round 01', concepts: previousLogoConcepts },
] as const

export const logoReferences = [
  { name: 'Papertrail', image: '/logos/references/papertrail.png', detail: 'Glass & edge light' },
  { name: 'Linear', image: '/logos/references/linear.png', detail: 'Geometry & depth' },
  { name: 'Ghost', image: '/logos/references/ghost.png', detail: 'Frost & softness' },
  { name: 'Pencil', image: '/logos/references/pencil.png', detail: 'Material & detail' },
] as const
