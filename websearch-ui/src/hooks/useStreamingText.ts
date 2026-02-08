import { useState, useEffect } from 'react'

interface UseStreamingTextOptions {
  speed?: number // ms per character
  delay?: number // initial delay before starting
}

interface UseStreamingTextReturn {
  displayed: string
  isComplete: boolean
}

export function useStreamingText(
  fullText: string,
  options?: UseStreamingTextOptions
): UseStreamingTextReturn {
  const { speed = 20, delay = 0 } = options ?? {}
  const [displayed, setDisplayed] = useState('')
  const [isComplete, setIsComplete] = useState(false)

  useEffect(() => {
    setDisplayed('')
    setIsComplete(false)

    if (!fullText) {
      setIsComplete(true)
      return
    }

    let i = 0
    let intervalId: number | undefined

    const timeoutId = setTimeout(() => {
      intervalId = window.setInterval(() => {
        if (i <= fullText.length) {
          setDisplayed(fullText.slice(0, i))
          i++
        } else {
          setIsComplete(true)
          if (intervalId) clearInterval(intervalId)
        }
      }, speed)
    }, delay)

    return () => {
      clearTimeout(timeoutId)
      if (intervalId) clearInterval(intervalId)
    }
  }, [fullText, speed, delay])

  return { displayed, isComplete }
}
