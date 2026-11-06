import { useRef, useState, type ReactNode } from 'react'
import setup from './assets/tutorial/setup.png'
import login from './assets/tutorial/login.png'
import workspace from './assets/tutorial/workspace.png'
import identity from './assets/tutorial/identity.png'
import term from './assets/tutorial/term.png'
import timetable from './assets/tutorial/timetable.png'
import calendar from './assets/tutorial/calendar.png'
import layout from './assets/tutorial/layout.png'
import rooms from './assets/tutorial/rooms.png'
import materials from './assets/tutorial/materials.png'
import voice from './assets/tutorial/voice.png'
import './Tutorial.css'

type Chapter = { id: string; group: string; title: string; keywords: string; content: ReactNode }
function Figure({ src, alt, note }: { src: string; alt: string; note: string }) {
  return <figure className="guide-figure"><a href={src} target="_blank" rel="noopener noreferrer" aria-label={`${alt}，打开大图`}><img src={src} alt={alt} loading="lazy" /></a><figcaption>{note} · 点击图片看大图</figcaption></figure>
}
function Path({ children }: { children: ReactNode }) { return <p className="guide-path"><strong>从这里进入：</strong>{children}</p> }
function Tip({ children }: { children: ReactNode }) { return <p className="guide-tip">{children}</p> }
const chapters: Chapter[] = [
  { id:'roadmap', group:'从零开始', title:'先看这里：配置路线', keywords:'顺序 安装 初始化 管理员 教师 新手 开始', content:<>
    <p className="guide-lead">第一次使用？按下面的顺序做。前面完成后，后面的选择才会出现。</p>
    <ol className="guide-route"><li><strong>安装并打开</strong><span>在教室电脑安装，打开设置。</span></li><li><strong>创建主管理员</strong><span>填写学校和登录账号，保存恢复码。</span></li><li><strong>设置班级和科目</strong><span>添加本班、要上的课，再绑定本机班级。</span></li><li><strong>添加老师</strong><span>为老师创建账号，分配任教班级和科目。</span></li><li><strong>安排课表</strong><span>建学期、填上课时间与课程，需要时设置调休。</span></li><li><strong>布置桌面</strong><span>选显示内容，拖动模块，保存排布。</span></li><li><strong>老师开始使用</strong><span>手机扫码选班，管理作业、资料和语音。</span></li></ol>
    <Tip>前 1～6 步由全局主管理员在教室电脑上操作。老师日常使用，从“手机连接与登录”或“老师工作台”开始。</Tip>
    <p>图里使用的是<strong>示例学校、示例班级和示例账号</strong>。红色编号与箭头用来指出按钮或填写位置，实际名称请以你的学校为准。</p>
    <Figure src={workspace} alt="教师工作台，标出全局设置、作业、资料和语音入口" note="①全局设置　②作业　③课堂资料　④语音播报" />
  </> },
  { id:'install', group:'从零开始', title:'1. 安装与打开', keywords:'安装包 解压 安装 桌面 开始菜单 托盘 设置 退出', content:<>
    <ol><li>在教室电脑上找到管理员提供的未来教室 <strong>.exe 安装程序</strong>，双击打开安装向导。</li><li>点<strong>立即安装</strong>，等待显示“安装完成”，再点<strong>打开未来教室</strong>。</li><li>从电脑桌面或 Windows 开始菜单打开“未来教室”。</li><li>在屏幕右下角找到未来教室图标，右键点击，选择<strong>设置</strong>，打开本机管理网页。看不到图标时，先点任务栏右下角的“∧”。</li></ol>
    <Tip>需要结束程序时，右键右下角图标选择“退出”。打开其他窗口或最大化软件时，桌面信息会被窗口挡住，回到桌面即可查看。</Tip>
  </> },
  { id:'setup', group:'从零开始', title:'2. 首次创建管理员', keywords:'初始化 密码 恢复码 学校 首次 账号 登录', content:<>
    <Path>教室电脑右下角图标 → 右键“设置” → 初始化未来教室</Path>
    <ol><li>在白板本机打开设置。首次使用时，页面显示“初始化未来教室”。</li><li>填写<strong>学校名称、管理员用户名、管理员显示名称</strong>。用户名是以后登录时输入的名字；显示名称是页面上展示的名字。</li><li>设置一个 12～128 个字符的密码，再输入一次确认。</li><li>点<strong>创建管理员并进入设置</strong>。如果页面显示恢复码，把它抄下并妥善保管，勾选已保存后点<strong>完成并隐藏恢复码</strong>。</li></ol>
    <Tip>恢复码只在生成时显示。忘记密码时要用它；不要把密码或恢复码发在班级群里。</Tip>
    <Figure src={setup} alt="首次初始化页面，标出学校名称和创建管理员按钮" note="①填写学校和管理员信息　②创建管理员" />
  </> },
  { id:'login', group:'从零开始', title:'3. 登录与账号权限', keywords:'登录 角色 权限 管理员 普通用户 忘记密码', content:<>
    <ol><li>打开教室管理网页，在“用户名”和“密码”中输入管理员给你的账号信息，点<strong>登录</strong>。</li><li>新建的老师账号首次登录时，按照页面提示修改初始密码。</li><li>在同一页面刷新或暂时离开一会儿，通常不用重复登录；关闭网页后再次打开，按提示重新登录。</li></ol>
    <p><strong>全局主管理员</strong>可以设置整台教室电脑；<strong>教师管理员</strong>可以管理分配给自己的班级和科目；<strong>普通用户</strong>不能使用教师管理功能。找不到班级或按钮点不了，请联系全局主管理员分配权限。</p>
    <Tip>本版没有老师自己注册的按钮。其他老师的账号由主管理员创建。</Tip>
    <Figure src={login} alt="登录页面，箭头指出用户名输入框和登录按钮" note="①输入用户名和密码　②点击登录" />
  </> },
  { id:'school', group:'从零开始', title:'4. 学校、班级与科目', keywords:'添加班级 年级 新科目 语文 数学 英语 绑定 本机', content:<>
    <Path>登录主管理员 → 全局设置 → 账号与学校</Path>
    <ol><li>在“学校与本机班级”填写学校名称，点<strong>保存学校名称</strong>。</li><li>在“班级管理”输入班级名称，例如“七年级一班”，点<strong>添加班级</strong>。每个班级添加一次。</li><li>回到“本机显示班级”，选择这台教室电脑所在的班，点<strong>绑定当前班级</strong>。</li><li>在本页“科目管理”填写科目名称、选择颜色，再保存。语文、数学、英语以外的课程也在这里添加。</li></ol>
    <Figure src={identity} alt="账号与学校页面，指出本机班级选择与添加班级" note="①选择并绑定本机班级　②输入班级名称后添加" />
    <Tip>班级绑定错了，桌面课表、资料和语音目标都可能不对。每台教室电脑都要绑定自己的班级。</Tip>
  </> },
  { id:'teachers', group:'从零开始', title:'5. 添加老师和任教范围', keywords:'新建账号 新增账号 教师管理员 任教范围 默认老师 分配科目', content:<>
    <Path>全局设置 → 账号与学校 → 账号与任教范围</Path>
    <ol><li>点<strong>新增账号</strong>，填写登录名、显示名称和初始密码，角色选择“教师管理员”。</li><li>在任教范围中选择该老师任教的<strong>班级和科目</strong>，逐项添加；例如“七年级一班＋语文”。</li><li>保存账号，把登录名和初始密码单独交给老师。老师首次登录时需要修改密码。</li><li>在“默认任课老师”中选择班级、科目和已分配该范围的老师，再保存。课表选择“默认老师”时会使用这里的设置。</li></ol>
    <Tip>同一位老师教多个班或多门课时，每个“班级＋科目”都要加上。主管理员交接需使用“账号安全”中的交接功能。</Tip>
  </> },
  { id:'semester', group:'从零开始', title:'6. 创建学期', keywords:'学期 创建 第一周周一 开始 结束', content:<>
    <Path>全局设置 → 课表与日历 → 添加学期</Path>
    <ol><li>展开<strong>添加学期</strong>，选择班级，填写学期名称、开始日期和结束日期。</li><li>填写<strong>第一周周一</strong>：选择该学期第一教学周的星期一，系统据此计算单双周。</li><li>点<strong>创建学期</strong>，在“当前学期”中选中它，确认“启用学期”已经打开。</li><li>如学校每周课表都一样，使用“单表”；单双周不同则点“启用单双周”，分别填写。</li></ol>
    <Figure src={term} alt="课表页面，指出添加学期和创建学期按钮" note="①展开添加学期　②填写后创建" />
  </> },
  { id:'schedule', group:'从零开始', title:'7. 填写每周课表', keywords:'上课时间 课间 节次 星期 删除课节 单双周 老师', content:<>
    <Path>全局设置 → 课表与日历 → 每周课表</Path>
    <ol><li>先选好学期和要编辑的周表。在“添加课节”填写名称、类型、开始与结束时间，点<strong>添加到当前周表</strong>。</li><li>回到“每周课表”，每一行代表一节课。按星期选择科目；老师可选“默认老师”“指定老师”或“待指定”。</li><li>需要休息时间时，添加课节并把类型选为“课间”“午休”等。时间格式是 <strong>08:00</strong>，按页面要求修改数字。</li><li>一行不需要时点左侧<strong>删除课节</strong>。编辑完后点<strong>保存设置</strong>，等页面提示保存完成。</li></ol>
    <Tip>切换学期或增删课表结构前，先保存已有修改。科目下拉框没有想要的课程时，到“账号与学校”的科目管理添加。</Tip>
    <Figure src={timetable} alt="每周课表页面，指出课节类型和删除课节" note="①删除这一行　②选择课程或课间类型；星期列里选择科目和老师" />
  </> },
  { id:'calendar', group:'从零开始', title:'8. 放假、调休和补课', keywords:'调休 调课 10月10日 10月14日 来源日期 补课 放假', content:<>
    <Path>全局设置 → 课表与日历 → 校历、调休与空档</Path>
    <ol><li>先看“采用的日历”，确认正在使用的是<strong>校内日历</strong>还是<strong>公共节假日日历</strong>，在当前使用的日历里设置。</li><li>在“日期”选<strong>实际要上课或放假的那一天</strong>，并选择所属学期。</li><li>放假选“休息”；补课选<strong>使用指定日期的课表</strong>，再在“课表来源日期”选择要照搬的那一天。</li><li>点<strong>添加 / 更新日期安排</strong>，再点本区域的<strong>保存设置</strong>。等保存成功，才算完成。</li></ol>
    <Tip>例：10 月 10 日上 10 月 14 日的课，就把“日期”选 10 月 10 日，“课表来源日期”选 10 月 14 日。不需要为了提前设置而修改电脑日期。“待确认”不代表已排课。</Tip>
    <Figure src={calendar} alt="校历页面，指出添加日期安排和保存设置" note="①添加日期安排　②保存整个日历设置" />
  </> },
  { id:'layout', group:'从零开始', title:'9. 摆放桌面模块', keywords:'画布 拖动 缩放 添加 删除 吸附 左 右 模块 组件', content:<>
    <Path>全局设置 → 桌面排布</Path>
    <ol><li>选择上方“添加模块”里的项目，点<strong>添加模块</strong>。</li><li>在画布里按住模块拖动，改变位置；选中后拖动右下角小方块，改变大小。模块靠近时会自动对齐并留一点空隙。</li><li>要贴屏幕边缘，点<strong>完全贴左</strong>或<strong>完全贴右</strong>。不需要的普通模块选中后点“删除所选项目”。</li><li>调整完点<strong>保存设置</strong>，回到 Windows 桌面查看效果。</li></ol>
    <Tip>“课堂内容”是随当前课程变化的整体区域，可以移动和缩放，需要保留。课间倒计时的位置由程序安排。只改画布但没有保存，不会应用到桌面。</Tip>
    <Figure src={layout} alt="单层桌面画布，指出添加模块和可拖动的画布区域" note="①选择要添加的模块　②在画布中拖动、缩放" />
  </> },
  { id:'appearance', group:'桌面和课程', title:'桌面外观、时钟与倒计时', keywords:'颜色 配色 透明 玻璃 字号 秒 时钟 日期 倒计时 显示器', content:<>
    <Path>全局设置 → 桌面与配色</Path>
    <ol><li>选择显示器和时钟样式；需要显示秒数时打开“时钟显示秒”。</li><li>选择深色或浅色文字，拖动滑块调整课程、作业和玻璃背景的深浅。</li><li>在“桌面组件”勾选需要显示的时钟、日期、课程表、作业和提醒，并调整字号或大小。</li><li>需要倒计时时填写标题和目标日期；留空目标日期可停用。修改后点<strong>保存设置</strong>。</li></ol><Tip>看不清文字时，先换文字颜色，再调整背景深浅。没有点保存，预览里的变化不会应用到桌面。</Tip>
  </> },
  { id:'subject-layout', group:'桌面和课程', title:'不同课程显示不同内容', keywords:'学科独立布局 班级 科目 老师 资料 关联', content:<>
    <Path>全局设置 → 学科独立布局</Path>
    <ol><li>依次选择<strong>班级、科目、任课老师</strong>，点“读取 / 创建该组合布局”。</li><li>开启该组合的独立布局，按页面调整项目和课堂资料的显示方式。</li><li>需要使用这位老师的科目资料时，点<strong>关联这位老师的科目资料</strong>。</li><li>点<strong>保存学科布局</strong>。只影响当前选择的班级、科目和老师组合。</li></ol><Tip>找不到老师时，先到“账号与学校”检查这位老师是否有该班该科目的任教范围。关闭独立布局后使用公共课程区域。</Tip>
  </> },
  { id:'text-images', group:'桌面和课程', title:'添加固定文字和图片', keywords:'文字 图片 TXT 节点 发布 页序 撤销', content:<>
    <Path>全局设置 → 文字与图片</Path>
    <ol><li>先在需要的布局中添加“文字内容”或“图片内容”模块，并保存布局。</li><li>在“文字与图片”选择公共布局或班级／科目／老师的独立布局，点<strong>读取该布局的内容节点</strong>，再选择要编辑的项目。</li><li>文字：直接输入，或导入 TXT 作为草稿，最后点<strong>保存文字</strong>。</li><li>图片：选择图片，按需要调整顺序，点<strong>发布图片与页序</strong>；要让白板显示某一页，选中后点“让白板显示此页”。</li></ol><Tip>选择文件只是放进草稿，保存文字或发布图片后才会应用。</Tip>
  </> },
  { id:'switch', group:'桌面和课程', title:'临时换一节课', keywords:'换课 临时换课 恢复原课 桌面 公开 不需要登录', content:<>
    <Path>教室电脑桌面 → 临时换课 ▾</Path>
    <ol><li>在正在上课时，点击桌面上的<strong>临时换课</strong>。</li><li>选择临时科目和临时任课教师，点<strong>保存本节临时换课</strong>。</li><li>要改回去，再点临时换课，选择<strong>恢复本节原课</strong>。</li></ol><Tip>只影响当前这一节课，下课自动结束，不会修改整周课表。休息、课间或当前没有确定课程时按钮不可用。没有可选老师时，页面会提示是否以“未分配老师”换课。</Tip>
    <p>时钟、日期和课程表用于查看；课堂资料条目和临时换课按钮可以直接点击，不需要登录或解锁。</p>
  </> },
  { id:'connect', group:'手机连接与登录', title:'手机扫码选择班级', keywords:'二维码 托盘 快捷连接 手机 wifi 微信 浏览器 收藏', content:<>
    <ol><li>手机先连接学校的 Wi-Fi。教室电脑右下角右键未来教室图标，选择<strong>快捷连接</strong>。</li><li>用手机扫描页面二维码；不方便扫码时，输入页面上显示的完整网址。</li><li>在“选择班级”页搜索学校或班级，点<strong>进入班级</strong>。首次进入其他班级时，先核对显示的班级和地址。</li><li>常用班级点星号收藏，以后可在“我的收藏”里找到。每台教室分别登录。</li></ol><Tip>微信扫一扫后，请在微信右上角选择“在浏览器中打开”。如果出现证书提示，请联系学校管理员按学校说明安装并信任证书，再登录。</Tip>
    <Figure src={rooms} alt="班级选择首页，指出添加到桌面、搜索和进入班级" note="①添加到桌面　②搜索　③进入对应班级" />
  </> },
  { id:'pwa', group:'手机连接与登录', title:'把入口放到手机桌面', keywords:'PWA iPhone 苹果 安卓 添加到主屏幕 Safari 安装应用', content:<>
    <ol><li>先按上一节扫码打开“选择班级”。也可以先不安装，直接使用网页。</li><li><strong>安卓：</strong>使用系统浏览器打开，点“添加到桌面”；出现安装提示时按提示确认。如果没有弹窗，在浏览器菜单找“安装应用”或“添加到主屏幕”。</li><li><strong>iPhone：</strong>使用 Safari 打开，点底部或顶部的<strong>分享</strong>按钮（方框向上箭头），选<strong>添加到主屏幕</strong>，再点“添加”。</li><li>以后从手机桌面上的未来教室图标打开，选择班级并登录。</li></ol><Tip>首次证书信任和添加到桌面是两件事。没有网络时可以看到已保存的班级入口；登录、查看课堂内容和发送语音仍需要学校网络及教室电脑在线。</Tip>
  </> },
  { id:'workbench', group:'老师日常使用', title:'认识老师工作台', keywords:'工作台 导航 作业 资料 提醒 语音', content:<>
    <p>登录后，点顶部导航进入需要的功能：</p><ul><li><strong>我的今日作业：</strong>填写今天的作业。</li><li><strong>我的课堂资料：</strong>上传、下载或删除课堂文件。</li><li><strong>今日提醒：</strong>给选定班级编辑共同提醒。</li><li><strong>语音播报：</strong>向当前教室发送录音。</li></ul><Tip>全局设置只对全局主管理员显示。老师只能管理管理员分配给自己的范围。</Tip>
    <Figure src={workspace} alt="教师工作台和顶部导航" note="①全局设置　②作业　③资料　④语音；今日提醒也在同一排" />
  </> },
  { id:'homework', group:'老师日常使用', title:'填写今天的作业', keywords:'作业 今日作业 班级 课程 老师 保存 清空', content:<>
    <Path>工作台 → 我的今日作业</Path><ol><li>在列表里选择<strong>班级、科目和内容所属老师</strong>对应的项目。</li><li>在正文框输入作业。可以换行，目前只支持文字。</li><li>点<strong>保存当天内容</strong>，看到成功提示后完成。如需删掉今天的作业，点“清空当天内容”并确认。</li><li>想确认白板显示情况，可点<strong>查询桌面显示状态</strong>。</li></ol><Tip>这里按教室电脑的日期记录“今天”。保存成功不等于此刻一定出现在桌面：还要看当前班级、当前课程和组件显示设置。</Tip>
  </> },
  { id:'materials', group:'老师日常使用', title:'上传和打开课堂资料', keywords:'课堂资料 文件 上传 班级 可见 下载 删除 PPT PDF 文档', content:<>
    <Path>工作台 → 我的课堂资料</Path><ol><li>选择<strong>老师和科目</strong>对应的资料目录。</li><li>点<strong>选择本机课堂文件</strong>，选择要上传的文件。可一次选择 1～20 个。</li><li>在“允许展示的任教班级”至少勾选一个班；需要时调整本批文件顺序。</li><li>先点<strong>确认顺序并上传整批</strong>，等待上传完成，再点<strong>校验并整批添加</strong>。页面确认成功后才算添加完成。</li><li>在文件列表可下载或删除。上课时，在桌面课堂资料上移动鼠标，单击文件即可打开。</li></ol>
    <Tip>支持 TXT、PDF、PPTX、DOCX、XLSX、PNG、JPEG 和 BMP。普通文件最多 50 MiB；图片最多 10 MiB。资料要匹配当前班级、科目和老师，并允许该班展示，才会出现在对应课堂内容中。</Tip>
    <Figure src={materials} alt="课堂资料页面，标出选文件、勾选班级、上传和校验添加" note="①选择文件　②勾班级　③上传　④校验并添加" />
  </> },
  { id:'voice', group:'老师日常使用', title:'给教室发送语音', keywords:'录音 话筒 麦克风 播报 次数 按住说话 取消', content:<>
    <Path>工作台 → 语音播报</Path><ol><li>先看“播报目标”，确认是正确的教室，状态为<strong>可发送</strong>。</li><li>按需要选择“总播放次数”。第一次使用时，浏览器会请求麦克风权限，请选择允许。</li><li>按住<strong>按住说话</strong>开始说话，松开后发送；向上滑再松开可取消。也可点“点击开始录音”，说完点“结束并发送”。</li><li>在“播报记录”查看排队和播放状态；需要停止时，点对应的停止按钮并确认。</li></ol>
    <Tip>说话至少 0.5 秒，最长时间以页面显示为准。已发送的语音不会因为关闭网页自动取消；电脑要在线且声音设备可用，才能播放。</Tip>
    <Figure src={voice} alt="语音播报页面，指出班级、播放次数和按住说话按钮" note="①核对班级　②选播放次数　③按住说话" />
  </> },
  { id:'reminders', group:'老师日常使用', title:'发布今日提醒', keywords:'提醒 共同提醒 班级 冲突 草稿', content:<>
    <Path>工作台 → 今日提醒</Path><ol><li>选择需要提醒的班级。</li><li>填写提醒内容，点<strong>保存当天内容</strong>；不需要时可以清空当天内容。</li><li>若页面提示内容已被别人修改，点“载入最新修订（保留草稿）”，对照内容后选择保留草稿或采用最新内容，再保存。</li></ol><Tip>同一班级的今日提醒由有权限的老师共同编辑，不是每个老师各有一份。页面提示提交结果未知时，先按页面提示查询原命令，避免重复提交。</Tip>
  </> },
  { id:'sound-policy', group:'管理员维护', title:'设置教室声音', keywords:'声音 播报 策略 限制 时长 次数', content:<>
    <Path>全局设置 → 声音播放策略</Path><ol><li>先确认教室电脑的音量和扬声器正常，点“检查 Windows 音频输出能力”。</li><li>根据需要调整播报时长、播放次数和排队限制，勾选“我已核对”，点“保存资源限制”。</li><li>需要开启或停用时，调整“启用声音播报”开关，勾选“我已核对”，再点“明确启用播报”或“明确停用播报”，并查看当前状态。</li></ol><Tip>启用播报需要输出能力检查通过。老师还需要在语音播报页面选择正确教室，并允许浏览器使用麦克风。</Tip>
  </> },
  { id:'network', group:'管理员维护', title:'查看连接和证书', keywords:'HTTPS IP 端口 网络 WiFi 防火墙 证书 信任', content:<>
    <Path>全局设置 → HTTPS与连接</Path><ol><li>先看“教师 HTTPS 地址”和“监听状态”。老师扫码或手动输入时使用这里显示的实际地址。</li><li>需要首次给手机配置信任时，请学校管理员提供并核对公开证书，再按手机系统的证书说明操作。</li><li>地址变化或确需调整时，由管理员检查 IP、端口、允许来源和证书，再按页面提示保存或生成配置。</li></ol><Tip>手机和教室电脑需要网络互通。证书信任不会自动应用到其他手机；更改地址或停止 HTTPS 可能让已连接的老师暂时断开。</Tip>
  </> },
  { id:'autostart', group:'管理员维护', title:'开机登录后自动启动', keywords:'开机 自启动 登录Windows 启用 关闭', content:<>
    <Path>全局设置 → 登录自启动</Path><ol><li>查看“期望状态”和“Windows 实际状态”。</li><li>希望以后登录 Windows 后自动打开时，点<strong>启用登录自启动</strong>；不需要时点“关闭登录自启动”。</li><li>状态对不上时，可点“按期望状态修复注册”，再重新读取状态。</li></ol><Tip>这个设置针对安装程序的 Windows 用户。电脑关机或没有登录时，程序不会自己运行。</Tip>
  </> },
  { id:'backup', group:'管理员维护', title:'备份与恢复', keywords:'备份 恢复 数据 转移 迁移 原件 初始化', content:<>
    <Path>在教室电脑本机登录全局主管理员 → 全局设置 → 备份与维护</Path>
    <p><strong>备份：</strong>点“创建完整备份”，等待完成，再点<strong>保存完整备份文件</strong>，把文件放到学校指定的安全位置。</p>
    <p><strong>恢复：</strong>选择完整备份文件和本安装／跨安装方式，点“建立任务并上传”，再点“只读预检此备份”。跨安装时按页面逐项对应班级、科目和老师，重新预检；仔细阅读影响范围，勾选确认后点<strong>明确确认恢复此备份</strong>。</p>
    <Tip>恢复会替换相应教学资料，确认前先看清范围。备份不包含账号口令和恢复码，请另外妥善保管。重新初始化会清理资料，不能当作无损找回密码。</Tip>
  </> },
  { id:'updates', group:'管理员维护', title:'查看版本和自动更新', keywords:'版本 更新 自动 下载 安装 更新包 局域网 关于', content:<>
    <ol><li>在教室电脑右下角右键未来教室图标，点<strong>关于</strong>，可查看当前版本和更新状态。</li><li>想立即检查时点<strong>立即更新</strong>，再看页面状态和版本号。</li><li>支持自动更新的新版安装后默认启用局域网更新。在一台电脑安装官方新版 <strong>.exe</strong> 后，它会自动保留并分享完整安装包，同范围内支持该更新协议的电脑可自动发现、下载、核验、安装并重新打开，无需另行导入更新包。1.0.0.1～1.0.0.3请先手动运行新版安装包；1.0.1.0可直接自动升级到1.0.2.0。</li><li>要调整模式：本机管理员进入<strong>软件更新</strong>，选择自动安装或仅提醒，再保存。独立学校组只是需要隔离范围时才设置的高级选项。</li></ol><Tip>安装时程序会短暂退出。没有电脑持有完整官方更新包、电脑不在线、网络隔离或防火墙阻挡时，其他电脑需要等条件恢复。已有关闭更新的设置不会被强行打开。</Tip>
  </> },
  { id:'security', group:'管理员维护', title:'修改密码与找回账号', keywords:'忘记密码 密码 恢复码 交接 管理员', content:<>
    <ol><li>已登录时，在工作台点<strong>修改密码</strong>，输入当前密码和两遍新密码后保存。</li><li>忘记密码但保存了恢复码时，在<strong>白板本机</strong>登录页点“忘记密码？使用恢复码”，输入用户名、恢复码和新密码。</li><li>成功后旧密码和旧恢复码会失效。页面显示新恢复码时，立即保存好。</li><li>需要更换全局主管理员时，到“账号与学校 → 账号安全”，选择已启用并完成首次改密的接任账号，核对提示并确认交接。</li></ol><Tip>没有恢复码时不要随意点“重新初始化”：它会清理原学校资料。请先联系负责设备的管理员，核对备份和处理方式。</Tip>
  </> },
  { id:'trouble', group:'遇到问题', title:'常见问题', keywords:'看不到课表 看不到资料 无声音 无法连接 无法登录 不显示 解决', content:<>
    <h3>桌面没有课表？</h3><p>检查本机是否绑定了正确班级；学期是否启用且日期在范围内；今天是否有课、校历是否设为休息或待确认；“桌面与配色”中课程表是否显示。课表和校历要各自保存。</p>
    <h3>上传资料后桌面没有？</h3><p>确认已经完成“上传”和“校验并整批添加”两步，允许当前班级显示；再核对当前科目、任课老师和资料组件。不同课程不会显示同一份老师资料。</p>
    <h3>手机连不上或提示证书？</h3><p>确认手机和教室电脑都在线、连接的学校网络互通；重新从托盘“快捷连接”获取地址。证书问题请联系学校管理员按学校要求配置手机信任；微信内请换系统浏览器。</p>
    <h3>语音发出后没声音？</h3><p>看“播报记录”是否完成，检查教室电脑音量、音频设备和管理员的“声音播放策略”。没有麦克风时检查浏览器权限、HTTPS 和是否使用系统浏览器。</p>
    <h3>没有班级、科目或功能按钮？</h3><p>联系主管理员检查账号角色、任教范围、科目启用状态和本机班级绑定。</p>
    <h3>页面提示保存结果未知？</h3><p>先按页面提供的查询或重新读取按钮核对结果，确认前不要重复提交。</p>
  </> },
]

export default function Tutorial() {
  const [current, setCurrent] = useState('roadmap'), [query, setQuery] = useState('')
  const content = useRef<HTMLDivElement>(null)
  const visible = chapters.filter(chapter => `${chapter.title} ${chapter.keywords}`.toLowerCase().includes(query.trim().toLowerCase()))
  const chapter = chapters.find(item => item.id === current) ?? chapters[0]!
  const index = chapters.indexOf(chapter)
  function open(id: string) { setCurrent(id); content.current?.scrollTo({ top: 0 }) }
  return <div className="guide-shell">
    <nav className="guide-nav" aria-label="教程目录"><label className="guide-search"><span>找教程</span><input type="search" value={query} onChange={event => setQuery(event.target.value)} placeholder="搜索作业、课表、语音…" /></label>
      {visible.length === 0 && <p className="guide-no-results">没有找到，换个词试试。</p>}
      {Array.from(new Set(visible.map(item => item.group))).map(group => <div key={group} className="guide-nav-group"><h3>{group}</h3>{visible.filter(item => item.group === group).map(item => <button key={item.id} type="button" className={item.id === chapter.id ? 'active' : ''} aria-current={item.id === chapter.id ? 'page' : undefined} onClick={() => open(item.id)}>{item.title}</button>)}</div>)}
    </nav>
    <div className="guide-content" ref={content}><article className="guide-article"><p className="guide-kicker">{chapter.group}</p><h2>{chapter.title}</h2>{chapter.content}</article><div className="guide-pagination"><button type="button" disabled={index === 0} onClick={() => open(chapters[index - 1]!.id)}>← 上一篇</button><span>{index + 1} / {chapters.length}</span><button type="button" disabled={index === chapters.length - 1} onClick={() => open(chapters[index + 1]!.id)}>下一篇 →</button></div></div>
  </div>
}
