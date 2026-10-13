import { afterEach } from 'vitest'
import { cleanup } from '@testing-library/react'

// Without globals, Testing Library doesn't unmount between tests by itself.
afterEach(cleanup)
