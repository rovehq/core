'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';

const tabs = [
  { href: '/', label: 'Home', icon: '⌂' },
  { href: '/tasks', label: 'Tasks', icon: '≣' },
  { href: '/audit', label: 'Audit', icon: '⌘' },
  { href: '/hooks', label: 'Hooks', icon: '⎇' },
  { href: '/memory', label: 'Memory', icon: '⛓' },
  { href: '/migrate', label: 'Migrate', icon: '⇪' },
  { href: '/starters', label: 'Starters', icon: '✦' },
  { href: '/browser', label: 'Browser', icon: '◎' },
  { href: '/voice', label: 'Voice', icon: '◍' },
  { href: '/brains', label: 'Brains', icon: '◉' },
  { href: '/policy', label: 'Policy', icon: '⟡' },
  { href: '/remote', label: 'Remote', icon: '⇄' },
  { href: '/channels', label: 'Channels', icon: '⌁' },
  { href: '/agents', label: 'Agents', icon: '◌' },
  { href: '/workflows', label: 'Workflows', icon: '⋯' },
  { href: '/approvals', label: 'Approvals', icon: '!' },
  { href: '/config', label: 'Config', icon: '⚙' },
  { href: '/settings', label: 'Settings', icon: '◈' },
  { href: '/plugins', label: 'Extensions', icon: '▣' },
];

export default function Nav() {
  const pathname = usePathname();

  return (
    <nav className="flex flex-wrap items-center gap-2 rounded-2xl border border-surface2/80 bg-surface/80 p-2 shadow-[0_18px_40px_rgba(0,0,0,0.22)] backdrop-blur">
      {tabs.map((tab) => {
        const isActive = pathname === tab.href;
        return (
          <Link
            key={tab.href}
            href={tab.href}
            className={`
              flex items-center gap-2 rounded-xl px-3 py-2 transition-all
              ${isActive 
                ? 'border border-primary/70 bg-primary/90 text-white shadow-[0_14px_28px_rgba(222,105,71,0.24)]'
                : 'border border-transparent text-gray-400 hover:border-surface2 hover:bg-surface2/80 hover:text-white'
              }
            `}
          >
            <span
              className={`
                inline-flex h-7 w-7 items-center justify-center rounded-full border text-xs
                ${isActive ? 'border-white/20 bg-white/10' : 'border-surface2 bg-background/60'}
              `}
            >
              {tab.icon}
            </span>
            <span className="hidden sm:inline">{tab.label}</span>
          </Link>
        );
      })}
    </nav>
  );
}
