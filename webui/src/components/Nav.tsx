'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';

const tabs = [
  { href: '/', label: 'Home', icon: '⌂' },
  { href: '/config', label: 'Config', icon: '⚙' },
  { href: '/settings', label: 'Settings', icon: '◈' },
  { href: '/plugins', label: 'Extensions', icon: '▣' },
];

export default function Nav() {
  const pathname = usePathname();

  return (
    <nav className="flex items-center gap-2 bg-surface rounded-lg p-2">
      {tabs.map((tab) => {
        const isActive = pathname === tab.href;
        return (
          <Link
            key={tab.href}
            href={tab.href}
            className={`
              flex items-center gap-2 px-4 py-2 rounded-md transition-all
              ${isActive 
                ? 'bg-primary text-white' 
                : 'text-gray-400 hover:text-white hover:bg-surface2'
              }
            `}
          >
            <span>{tab.icon}</span>
            <span className="hidden sm:inline">{tab.label}</span>
          </Link>
        );
      })}
    </nav>
  );
}
