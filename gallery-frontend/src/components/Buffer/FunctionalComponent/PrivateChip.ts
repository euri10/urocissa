import { h, type FunctionalComponent } from 'vue'
import { VChip, VIcon } from 'vuetify/components'

const PrivateChip: FunctionalComponent = () =>
  h(
    VChip,
    {
      'data-testid': 'private-photo-badge',
      'aria-label': 'Private photo',
      role: 'img',
      title: 'Private',
      color: 'surface',
      variant: 'flat',
      size: 'small',
      density: 'comfortable',
      link: false,
      class: 'position-absolute bottom-0 left-0 ma-2 px-1',
      // Match thumbnail overlay layering without intercepting selection/open gestures.
      style: { zIndex: 4, pointerEvents: 'none' }
    },
    () => h(VIcon, { icon: 'mdi-lock-outline', size: 'small' })
  )

export default PrivateChip
