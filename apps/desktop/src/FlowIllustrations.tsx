export function FlowArrow() {
  return <svg className="flow-arrow" viewBox="0 0 48 16" fill="none" aria-hidden="true">
    <path d="M2 8h42m-6-5 6 5-6 5" />
  </svg>;
}

export function MicrophoneIllustration() {
  return <svg className="flow-illustration" viewBox="0 0 160 112" fill="none" aria-hidden="true">
    <path className="illustration-wash" d="M31 41C41 13 100 5 121 28s18 66-14 74S14 86 31 41Z" />
    <g className="illustration-ink" strokeLinecap="round" strokeLinejoin="round">
      <path className="illustration-shadow" d="M52 102h58" />
      <g transform="rotate(-12 80 53)">
        <rect className="illustration-surface" x="64" y="16" width="32" height="57" rx="16" />
        <path d="M73 29h14m-16 8h18m-18 8h18m-16 8h14" />
        <path d="M56 54v4a24 24 0 0 0 48 0v-4M80 82v18" />
        <path className="illustration-accent" d="M74 65h12" />
      </g>
      <path d="M66 101h32" />
      <g className="illustration-noise">
        <path d="m27 28 4-6 4 8 4-6M120 53l5-5-2 10 6-4M40 77l-5 4m7 3-1 5M111 17v7m-3-4h6" />
        <circle cx="24" cy="56" r="2" /><circle cx="133" cy="31" r="2" />
      </g>
    </g>
  </svg>;
}

export function FilterIllustration({ reducing }: { reducing: boolean }) {
  return <svg className="flow-illustration" viewBox="0 0 160 112" fill="none" aria-hidden="true">
    <path className="illustration-wash" d="M22 67C15 35 43 10 85 13s57 15 53 49-35 41-66 39S27 91 22 67Z" />
    <g className="illustration-ink" strokeLinecap="round" strokeLinejoin="round">
      <path className="illustration-shadow" d="M47 101h70" />
      <path className="filter-side" d="m109 24 11 10v57l-11 9-64-10V32Z" />
      <rect className="filter-face" x="38" y="19" width="72" height="73" rx="16" />
      <path className="filter-slot" d="M45 49v14m58-14v14" />
      <g className={reducing ? "filter-mark is-reducing" : "filter-mark"} transform="rotate(18 75 51)">
        <path d="M62 48v15m12-30v32m12-24v22" />
      </g>
      <path className="filter-detail" d="M65 80h18" />
      <path className="illustration-noise" d="m17 50 4-4 3 9 5-6m-10 18 5-3" />
      {reducing ? <path className="illustration-accent" d="M125 56h17m-5-20v8m-4-4h8" />
        : <path className="illustration-noise" d="m126 50 4-4 3 9 5-6m-8 18 5-3" />}
    </g>
  </svg>;
}

export function CallIllustration({ headphones, reducing }: { headphones: boolean; reducing: boolean }) {
  return <svg className="flow-illustration" viewBox="0 0 160 112" fill="none" aria-hidden="true">
    <path className="illustration-wash" d="M30 24C53 10 112 15 130 39s9 53-18 62-54-8-72-21S7 38 30 24Z" />
    <g className="illustration-ink" strokeLinecap="round" strokeLinejoin="round">
      <path className="illustration-shadow" d="M42 102h82" />
      {headphones ? <>
        <path className="illustration-surface" d="M46 64V51a34 34 0 0 1 68 0v13h-8V51a26 26 0 0 0-52 0v13Z" />
        <rect className="filter-face" x="40" y="59" width="19" height="34" rx="8" />
        <rect className="filter-face" x="101" y="59" width="19" height="34" rx="8" />
        <path className="illustration-accent" d="M48 68v15m64-15v15" />
      </> : <g transform="rotate(5 80 59)">
        <rect className="illustration-surface" x="30" y="21" width="100" height="72" rx="10" />
        <path d="M30 37h100M64 102h32M80 93v9" />
        <path className="window-dots" d="M40 29h1m6 0h1m6 0h1" />
        <rect className="call-mic-tile" x="58" y="45" width="44" height="40" rx="10" />
        <rect className="illustration-accent" x="75" y="51" width="10" height="17" rx="5" />
        <path className="illustration-accent" d="M71 63v2a9 9 0 0 0 18 0v-2m-9 11v5m-4 0h8" />
      </g>}
      {reducing ? <path className="illustration-accent" d="m132 15 2 6 6 2-6 2-2 6-2-6-6-2 6-2ZM23 70v6m-3-3h6" /> : null}
    </g>
  </svg>;
}

export function CallSourceIllustration() {
  return <svg className="flow-illustration" viewBox="0 0 160 112" fill="none" aria-hidden="true">
    <path className="illustration-wash" d="M25 33C37 13 109 6 132 31s11 59-20 69S34 93 25 73 17 49 25 33Z" />
    <g className="illustration-ink" strokeLinecap="round" strokeLinejoin="round">
      <path className="illustration-shadow" d="M38 102h85" />
      <g transform="rotate(-5 80 58)">
        <rect className="illustration-surface" x="29" y="22" width="102" height="70" rx="10" />
        <path d="M29 38h102M64 101h32M80 92v9" />
        <path className="window-dots" d="M39 30h1m6 0h1m6 0h1" />
        <rect className="call-mic-tile" x="38" y="46" width="38" height="36" rx="7" />
        <circle cx="57" cy="58" r="5" />
        <path d="M47 74a10 10 0 0 1 20 0" />
        <path className="illustration-accent" d="M86 61v7m7-15v23m7-18v13m7-18v23m7-15v7" />
      </g>
      <path className="illustration-noise" d="m14 51 4-5 3 9 5-6M135 34l5-4-2 9 6-4" />
    </g>
  </svg>;
}

export function PlaybackIllustration({ reducing }: { reducing: boolean }) {
  return <svg className="flow-illustration" viewBox="0 0 160 112" fill="none" aria-hidden="true">
    <path className="illustration-wash" d="M27 37C43 12 105 8 126 33s17 59-11 69-70-8-86-26-12-24-2-39Z" />
    <g className="illustration-ink" strokeLinecap="round" strokeLinejoin="round">
      <path className="illustration-shadow" d="M32 102h98" />
      <path className="illustration-surface" d="M35 65V51a32 32 0 0 1 64 0v14h-8V51a24 24 0 0 0-48 0v14Z" />
      <rect className="filter-face" x="29" y="59" width="19" height="34" rx="8" />
      <rect className="filter-face" x="86" y="59" width="19" height="34" rx="8" />
      <path className="illustration-accent" d="M37 68v15m58-15v15" />
      <rect className="illustration-surface" x="112" y="53" width="24" height="46" rx="5" />
      <circle cx="124" cy="64" r="3" /><circle className="illustration-accent" cx="124" cy="83" r="7" />
      {reducing ? <path className="illustration-accent" d="m125 23 2 6 6 2-6 2-2 6-2-6-6-2 6-2Z" /> : null}
    </g>
  </svg>;
}
