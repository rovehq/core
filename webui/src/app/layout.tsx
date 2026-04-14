import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Rove Control Plane",
  description: "Daemon-first control plane for agents, remotes, approvals, and runtime operations.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body className="app-body">{children}</body>
    </html>
  );
}
