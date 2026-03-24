# Rove Feature Inventory And Competitor Feature Map

Date: March 25, 2026

Primary inputs:

- [`ROVE_ACTUAL_STATE_2026-03-24.md`](./ROVE_ACTUAL_STATE_2026-03-24.md)
- [`COMPETITIVE_INTELLIGENCE_ROVE_V2.md`](./COMPETITIVE_INTELLIGENCE_ROVE_V2.md)
- [`ROVE_LINK_MATRIX_2026-03-25.md`](./ROVE_LINK_MATRIX_2026-03-25.md)

This report separates three things clearly:

1. what Rove actually ships now
2. what is partial or missing in Rove
3. what features the competitor ecosystem already has, category by category

The goal is to stop mixing current product truth with roadmap ambition.

## Executive Summary

Rove already ships the infrastructure core:

- daemon runtime
- node identity and trust
- approvals and secrets
- signed remote execution
- ZeroTier transport surface
- signed extension catalog
- first-class `AgentSpec` and `WorkflowSpec`
- early agent factory
- command center
- first productized Telegram channel pack

The ecosystem still leads on product surface:

- installation and onboarding
- channels
- mobile shells
- voice
- ops tooling
- memory specialization
- ready-made workflow packs
- marketplace density

Rove is strongest where others are weakest:

- security and trust inside the runtime
- explicit specs instead of hidden prompt state
- signed remote execution and node identity
- unified daemon/CLI/API/WebUI control

Rove is weakest where others are strongest:

- distribution
- fast first-run experience
- polished assistant UX
- ecosystem breadth

## Part 1: Rove Features That Actually Ship

### 1. Core Runtime

1. One authenticated local daemon runtime exists.
2. One daemon binary supports both `desktop` and `headless` profiles.
3. CLI, daemon, and WebUI share the same control plane.
4. Hosted WebUI talks to the daemon instead of being a separate runtime.

### 2. Identity, Trust, And Security

5. Every node has a stable `node_id`.
6. `node_name` is editable without changing trust identity.
7. Remote trust is tied to Rove identity, not just address metadata.
8. Signed remote requests are implemented.
9. Replay protection is implemented.
10. Approval modes are implemented.
11. Approval rules and pending approvals are implemented.
12. Secret backend selection exists.
13. Vault-backed secret persistence exists.

### 3. Remote And Mesh Features

14. Peer pairing exists.
15. Explicit peer trust exists.
16. Remote status surfaces exist.
17. Remote send flow exists.
18. ZeroTier is exposed as an official remote transport surface.
19. ZeroTier install/setup/join/status/refresh surfaces exist.
20. ZeroTier discovery/pair/trust surfaces exist.
21. ZeroTier transport records and discovery cache exist.

### 4. Capability And Extension Layer

22. Builtin core tool precedence is protected.
23. Filesystem, terminal, and screenshot native adapters were fixed.
24. Persisted config is versioned.
25. SDK config view is typed and version-aware.
26. Extension catalog has a public default source.
27. Trust badges and provenance are surfaced.
28. Update availability is surfaced.
29. Developer-mode gating exists for unverified flows.

### 5. Agent And Workflow Runtime

30. `AgentSpec` exists as a first-class runtime object.
31. `WorkflowSpec` exists as a first-class runtime object.
32. Default assistant is represented as a spec.
33. Persistent spec storage exists.
34. Agent and workflow run history is stored in SQLite.
35. CLI/API/WebUI management for agents exists.
36. CLI/API/WebUI management for workflows exists.

### 6. Agent Factory

37. Agent factory preview exists.
38. Agent factory create exists.
39. Workflow factory preview exists.
40. Workflow factory create exists.
41. Template listing exists.
42. Task-to-agent conversion exists.
43. Task-to-workflow conversion exists.
44. Generated specs are explicit and disabled by default.

### 7. Command Center And UI

45. `/` is now a command center, not only a task box.
46. Aggregated overview API exists.
47. Recent tasks are shown.
48. Recent agent/workflow runs are shown.
49. Approvals panel exists.
50. Channels panel exists.
51. Services panel exists.
52. Remote snapshot panel exists.
53. Extension update snapshot exists.
54. Bounded daemon log tail exists.

### 8. Channel Runtime

55. `rove channel ...` is a real runtime noun.
56. Telegram status/setup/enable/disable/test/doctor exists.
57. Telegram has a dedicated Channels page in WebUI.
58. Default Telegram handler binds to a real enabled `AgentSpec`.

### 9. Practical Reliability Improvements

59. Daemon startup surfaces real data-dir errors.
60. Hosted WebUI daemon port handling was corrected.
61. Port override is user-configurable.
62. Builtin core tools were restored as authoritative.
63. Native tool reactor/runtime dependency bugs were fixed.

## Part 2: Rove Features That Are Partial

### 10. Telegram Depth

64. Telegram setup/status/test/doctor is real.
65. Real end-to-end inbound validation still depends on live bot credentials.
66. Approval/admin-chat behavior is only partially realized in the current path.
67. Richer multi-agent Telegram routing is not done.

### 11. Command Center Depth

68. Command center is operational.
69. It is not yet a full streaming operator console.
70. It is not yet a full fleet dashboard.
71. Drill-down and live observability remain shallow.

### 12. Agent Factory Depth

72. Factory is structured and explicit.
73. It is still mostly template-driven.
74. There is no full LLM-backed requirement compiler yet.
75. There is no graph-to-agent or graph-to-workflow path yet.
76. There is no full review/approval workflow around generated specs yet.

## Part 3: Rove Features That Are Still Missing

### 13. Product Surfaces Not Yet Shipped

77. Visual Agent Studio canvas
78. Slack pack
79. Discord pack
80. broader multi-channel routing
81. polished mobile shell
82. voice story
83. public docs site
84. one-click distribution story
85. OpenClaw migration tooling
86. richer ops toolkit
87. large public marketplace with published plugins

### 14. Platform And Ops Gaps

88. Windows service-install finish
89. backup and restore tooling
90. watchdog/self-heal flow
91. richer observability and tracing UI
92. broader deployment bundles

## Part 4: Competitor Feature Inventory

This section is not a per-repo scorecard. It is the combined feature surface the
ecosystem already demonstrates. Per-link repo detail lives in
[`ROVE_LINK_MATRIX_2026-03-25.md`](./ROVE_LINK_MATRIX_2026-03-25.md).

### 15. Distribution And Installation Features Competitors Already Have

93. Homebrew install
94. one-line curl installer
95. npm global install
96. Docker install
97. AppImage/DEB/RPM packaging
98. DMG/ZIP/NSIS desktop packaging
99. one-click Hugging Face Spaces deployment
100. Cloudflare Workers deployment
101. Nix packaging
102. Kubernetes/Helm/operator deployment
103. VPS control-panel deployment
104. offline regional installer bundles

Examples named in the reports:

- Moltis: Homebrew, curl install
- ZeroClaw: install script
- OpenClaw: npm install
- HuggingClaw: Hugging Face Spaces
- moltworker: Cloudflare Workers
- nix-openclaw: Nix
- openclaw-kubernetes / k8s-operator: Kubernetes
- 1Panel: VPS control panel
- u-claw: offline China bundle

### 16. Onboarding, Doctor, And Migration Features

105. interactive onboarding wizard
106. system diagnostics / `doctor`
107. auto-repair helpers
108. OpenClaw workspace import
109. translated/localized setup guides
110. beginner tutorials and use-case libraries

Examples:

- ZeroClaw: `onboard`, `doctor`
- openclaw-guardian: self-repair direction
- Moltis and ZeroClaw: OpenClaw migration
- Chinese translation/tutorial/use-case repos

### 17. Runtime And Performance Features

111. ultra-small static binaries
112. very low memory idle footprints
113. sub-100ms or even sub-10ms startup stories
114. edge-device deployment focus
115. Raspberry Pi friendliness
116. built-in supervisor / restart behavior
117. cron-style scheduling

Examples:

- ZeroClaw: tiny binary / low RAM / fast cold start
- Moltis: optimized Rust single binary
- PicoClaw: low-cost hardware focus

### 18. Security And Auth Features In Competitors

118. WASM sandboxing
119. Docker/Podman/Apple Container isolation
120. Wasmtime sandbox
121. capability-based permissions
122. explicit allowlists
123. authenticated pairing
124. WebAuthn passkey auth
125. external red-team and security scanners
126. supply-chain / prompt-injection scanning

Examples:

- IronClaw: WASM security depth
- Moltis: container plus Wasmtime sandboxing, WebAuthn
- ZeroClaw: allowlists and pairing
- aguara / AI-Infra-Guard: external scanning and red-team tooling

### 19. Memory And Retrieval Features

127. hybrid vector + full-text search
128. temporal memory graphs
129. long-term memory products
130. skill memory reuse
131. provenance-aware retrieval
132. receipt-style local memory
133. hybrid recall sidecars
134. dedicated memory operating-system framing

Examples:

- Moltis: hybrid vector + full-text
- mem0: personalized memory layer
- graphiti: temporal context graph
- MemOS / memU / EverMemOS / MoltBrain / supermemory / nocturne_memory

### 20. Agent And Workflow Orchestration Features

135. role-based agent companies
136. manager-worker systems
137. self-organizing multi-agent teams
138. sub-agent delegation
139. nesting depth limits
140. audit trails
141. typed pipelines
142. resumable jobs
143. workflow shells
144. durable background jobs
145. fleet orchestration dashboards

Examples:

- hiclaw, OpenMOSS, opencrew, edict, openlegion
- Moltis: sub-agent delegation
- lobster: typed pipelines / approvals / resumable jobs
- mission-control and similar ops dashboards

### 21. Channel And Communications Features

146. Telegram
147. Slack
148. Discord
149. WhatsApp
150. WeChat
151. Feishu/Lark
152. DingTalk
153. QQ / OneBot
154. WeCom
155. KakaoTalk
156. Twilio SMS/MMS/RCS
157. Meshtastic / LoRa
158. SimpleX
159. Urbit / Tlon
160. Satori multi-protocol adapter
161. ACP channel
162. ZTM / zero-trust messaging channel
163. Open WebUI channel bridge

The major ecosystem gap for Rove is still here: competitors and the OpenClaw
ecosystem already cover a broad messaging surface while Rove only productizes
Telegram today.

### 22. Assistant UX And Client Features

164. desktop AI studio UX
165. 300+ assistant presets
166. virtual office / ambient UI
167. desktop companion shells
168. pet/character interfaces
169. non-technical desktop onboarding wrappers
170. Android runtime shells
171. iOS app presence
172. push notifications
173. phone automation shells

Examples:

- Cherry Studio: polished desktop/preset-heavy UX
- Star-Office-UI / kkclaw: experiential shells
- MeowHub / botdrop-android / openclaw-termux: Android
- Moltis and OpenClaw family: mobile directions

### 23. Voice And Multimodal Features

174. built-in TTS/STT
175. multiple voice providers
176. phone and mobile-notification workflows
177. camera/device integration on mobile
178. signed-in browser attach flows

Examples from the reports:

- Moltis: 15+ TTS/STT providers
- OpenClaw current state: mobile device actions, notifications, camera/device APIs, Chrome DevTools MCP attach mode

### 24. Coding-Specific Features

179. coding-agent-first UX
180. IDE/worktree-centric workflows
181. Paper2Code / Text2Web / Text2Backend flows
182. computer-use and desktop automation
183. coding cowork dashboards
184. cross-tool coding wrappers for Codex/Claude/OpenClaw/Gemini

Examples:

- OpenHands
- DeepCode
- codeg
- paperclip
- cc-switch
- AionUi

### 25. Ops And Observability Features

185. Prometheus metrics
186. OpenTelemetry export
187. command-center dashboards
188. live observability
189. cost tracking
190. kanban / cron / activity logs
191. self-heal and rollback tools
192. backup and restore tools
193. fleet orchestration views

Examples:

- Moltis: Prometheus + OTel
- mission-control / clawport-ui / clawmetry / guardian / backup

### 26. Marketplace, Skills, And Ecosystem Features

194. public skill registries
195. versioning
196. vector-search discovery
197. CLI install
198. large public skill collections
199. cross-agent skill managers
200. translated skill libraries
201. domain-specific skill packs

Examples:

- ClawHub
- OpenAI skills
- MCP registry and server directories
- skillshare / ask / awesome-skill repos
- medical, bioinformatics, search, creator, research packs

### 27. Domain Workflow Packs

202. research packs
203. creator-growth and publishing packs
204. finance/trading packs
205. medical/biomedical packs
206. bioinformatics packs
207. search and knowledge distillation packs
208. agile-method packs

This is one of the largest gaps between Rove and the ecosystem. Competitors
already have many domain packs; Rove has the runtime but not the content layer.

## Part 5: Rove Versus Competitor Feature Classes

### 28. Where Rove Is Ahead Right Now

209. node identity as a first-class runtime concern
210. signed remote execution
211. replay protection
212. approvals and allowlist policy in the runtime
213. vault-backed secrets
214. versioned config
215. first-class versioned agent/workflow specs
216. explicit agent/workflow generation outputs
217. signed extension catalog model
218. builtin core tool precedence protection
219. one daemon/CLI/API/WebUI control plane
220. ZeroTier as a maintained remote transport surface

### 29. Where Rove Is Behind Right Now

221. install and distribution
222. onboarding and doctor experience
223. public documentation
224. OpenClaw migration path
225. voice
226. mobile apps and mobile shells
227. channel breadth
228. ops and observability depth
229. marketplace density
230. memory specialization
231. polished assistant UX
232. domain workflow packs
233. backup/restore/self-heal tooling

### 30. Where Rove Is Roughly At Par Or Directionally Competitive

234. local-first Rust runtime direction
235. single-binary infrastructure story
236. remote-node orchestration direction
237. early command-center direction
238. early channel productization direction
239. structured agent/workflow control model

## Part 6: Recommended Reading Order

1. Read this file for the feature inventory.
2. Read [`ROVE_ACTUAL_STATE_2026-03-24.md`](./ROVE_ACTUAL_STATE_2026-03-24.md) for current shipped truth.
3. Read [`COMPETITIVE_INTELLIGENCE_ROVE_V2.md`](./COMPETITIVE_INTELLIGENCE_ROVE_V2.md) for strategic threat framing.
4. Read [`ROVE_LINK_MATRIX_2026-03-25.md`](./ROVE_LINK_MATRIX_2026-03-25.md) for per-link repo comparison.

## Bottom Line

Rove already has the hardest infrastructure layer.

Competitors already have the broadest surface layer.

So the practical product job is not to reinvent the runtime. It is to add the
missing product surfaces on top of the runtime Rove already has:

- distribution
- migration
- channels
- marketplace density
- domain packs
- mobile and voice
- ops polish

That is the shortest path from "good infrastructure" to "replace-all product."
