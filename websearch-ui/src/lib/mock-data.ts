import type { SearchResult, AnswerModel } from './types'

export const MOCK_RESULTS: SearchResult[] = [
  {
    id: '1',
    title: 'Introduction to React Hooks - Official Docs',
    url: 'https://react.dev/reference/react/hooks',
    displayDomain: 'react.dev',
    snippet:
      'Hooks let you use state and other React features without writing a class. They let you use more of React features from function components.',
    publishedAt: '2024-01-15',
    faviconUrl: 'https://react.dev/favicon.ico',
    score: 0.98,
  },
  {
    id: '2',
    title: 'A Complete Guide to useEffect - Dan Abramov',
    url: 'https://overreacted.io/a-complete-guide-to-useeffect/',
    displayDomain: 'overreacted.io',
    snippet:
      'useEffect lets you synchronize things outside of the React tree according to our current props and state. Effects run after every render by default.',
    publishedAt: '2023-08-20',
    faviconUrl: 'https://overreacted.io/favicon.ico',
    score: 0.95,
  },
  {
    id: '3',
    title: 'React Hooks Tutorial – useState, useEffect, and How to Create Custom Hooks',
    url: 'https://www.freecodecamp.org/news/react-hooks-tutorial/',
    displayDomain: 'freecodecamp.org',
    snippet:
      'Learn how to use React Hooks in your projects. This tutorial covers useState, useEffect, useContext, useReducer, and how to create your own custom hooks.',
    publishedAt: '2024-02-10',
    faviconUrl: 'https://www.freecodecamp.org/favicon.ico',
    score: 0.92,
  },
  {
    id: '4',
    title: 'Rules of Hooks – React Documentation',
    url: 'https://react.dev/reference/rules/rules-of-hooks',
    displayDomain: 'react.dev',
    snippet:
      'Hooks are JavaScript functions, but you need to follow two rules when using them. Only call Hooks at the top level. Only call Hooks from React functions.',
    publishedAt: '2024-01-10',
    faviconUrl: 'https://react.dev/favicon.ico',
    score: 0.90,
  },
  {
    id: '5',
    title: 'Understanding React Hooks - Stack Overflow Blog',
    url: 'https://stackoverflow.blog/2021/10/react-hooks-guide/',
    displayDomain: 'stackoverflow.blog',
    snippet:
      'React Hooks were introduced in React 16.8. They allow developers to use state and lifecycle methods in functional components without using classes.',
    publishedAt: '2023-10-05',
    faviconUrl: 'https://stackoverflow.com/favicon.ico',
    score: 0.88,
  },
]

export const MOCK_DISCUSSIONS: SearchResult[] = [
  {
    id: 'd1',
    title: 'When should I use useMemo and useCallback? - r/reactjs',
    url: 'https://reddit.com/r/reactjs/comments/abc123',
    displayDomain: 'reddit.com/r/reactjs',
    snippet:
      'I see these hooks everywhere but I\'m not sure when to actually use them. Is it worth memoizing everything or is that premature optimization?',
    score: 0.85,
  },
  {
    id: 'd2',
    title: 'useEffect cleanup function not working as expected',
    url: 'https://stackoverflow.com/questions/12345678',
    displayDomain: 'stackoverflow.com',
    snippet:
      'My cleanup function runs on every render instead of just on unmount. What am I doing wrong with my dependency array?',
    score: 0.82,
  },
]

export const MOCK_ANSWER: AnswerModel = {
  shortAnswer:
    'React Hooks are functions that let you use state and lifecycle features in functional components without writing classes. The most common hooks are useState for managing state and useEffect for side effects.',
  keyPoints: [
    'useState lets you add state to functional components. Call it with an initial value and it returns [currentState, setterFunction].',
    'useEffect runs side effects after render. It replaces componentDidMount, componentDidUpdate, and componentWillUnmount.',
    'Custom hooks let you extract and reuse stateful logic between components. They must start with "use".',
    'Hooks must be called at the top level of your component, never inside loops or conditions.',
    'useCallback and useMemo help optimize performance by memoizing functions and computed values.',
  ],
  concepts: ['useState', 'useEffect', 'useCallback', 'useMemo', 'Custom Hooks', 'Rules of Hooks'],
  citations: {
    0: ['1', '3'],
    1: ['2', '3'],
    2: ['3'],
    3: ['4'],
    4: ['5'],
  },
  followUps: [
    'What is the difference between useEffect and useLayoutEffect?',
    'How do I share state between components with hooks?',
    'When should I use useReducer instead of useState?',
  ],
  caveats: [
    'Hooks only work in functional components, not class components',
    'The dependency array in useEffect requires careful management to avoid bugs',
  ],
  confidence: 'high',
}

export async function simulateSearch(query: string): Promise<{
  results: SearchResult[]
  discussions: SearchResult[]
  answer: AnswerModel
}> {
  // Simulate network delay
  await new Promise((resolve) => setTimeout(resolve, 1500))

  return {
    results: MOCK_RESULTS,
    discussions: MOCK_DISCUSSIONS,
    answer: {
      ...MOCK_ANSWER,
      shortAnswer: `Here's what I found about "${query}": ${MOCK_ANSWER.shortAnswer}`,
    },
  }
}
