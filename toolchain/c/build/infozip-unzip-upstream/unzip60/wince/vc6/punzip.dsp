# Microsoft Developer Studio Project File - Name="punzip" - Package Owner=<4>
# Microsoft Developer Studio Generated Build File, Format Version 6.00
# ** DO NOT EDIT **

# TARGTYPE "Win32 (WCE x86em) Application" 0x7f01
# TARGTYPE "Win32 (x86) Application" 0x0101
# TARGTYPE "Win32 (WCE SH3) Application" 0x8101
# TARGTYPE "Win32 (WCE MIPS) Application" 0x8201

CFG=punzip - Win32 Debug
!MESSAGE This is not a valid makefile. To build this project using NMAKE,
!MESSAGE use the Export Makefile command and run
!MESSAGE 
!MESSAGE NMAKE /f "punzip.mak".
!MESSAGE 
!MESSAGE You can specify a configuration when running NMAKE
!MESSAGE by defining the macro CFG on the command line. For example:
!MESSAGE 
!MESSAGE NMAKE /f "punzip.mak" CFG="punzip - Win32 Debug"
!MESSAGE 
!MESSAGE Possible choices for configuration are:
!MESSAGE 
!MESSAGE "punzip - Win32 Release" (based on "Win32 (x86) Application")
!MESSAGE "punzip - Win32 Debug" (based on "Win32 (x86) Application")
!MESSAGE "punzip - Win32 ANSI Release" (based on "Win32 (x86) Application")
!MESSAGE "punzip - Win32 ANSI Debug" (based on "Win32 (x86) Application")
!MESSAGE "punzip - Win32 (WCE x86em) Release" (based on "Win32 (WCE x86em) Application")
!MESSAGE "punzip - Win32 (WCE x86em) Debug" (based on "Win32 (WCE x86em) Application")
!MESSAGE "punzip - Win32 (WCE MIPS) Release" (based on "Win32 (WCE MIPS) Application")
!MESSAGE "punzip - Win32 (WCE MIPS) Debug" (based on "Win32 (WCE MIPS) Application")
!MESSAGE "punzip - Win32 (WCE SH3) Release" (based on "Win32 (WCE SH3) Application")
!MESSAGE "punzip - Win32 (WCE SH3) Debug" (based on "Win32 (WCE SH3) Application")
!MESSAGE 

# Begin Project
# PROP AllowPerConfigDependencies 0
# PROP Scc_ProjName ""
# PROP Scc_LocalPath ""
# PROP WCE_FormatVersion "6.0"

!IF  "$(CFG)" == "punzip - Win32 Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "Release"
# PROP BASE Intermediate_Dir "Release"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "Release"
# PROP Intermediate_Dir "Release"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /W3 /GX /O2 /D "WIN32" /D "NDEBUG" /D "_WINDOWS" /YX /FD /c
# ADD CPP /nologo /MT /W3 /GX /O2 /I "../.." /D "NDEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "_MBCS" /D "UNICODE" /YX /FD /c
# SUBTRACT CPP /X /Fr
MTL=midl.exe
# ADD BASE MTL /nologo /D "NDEBUG" /mktyplib203 /win32
# ADD MTL /nologo /D "NDEBUG" /mktyplib203 /win32
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d "NDEBUG"
# ADD RSC /l 0x409 /d "NDEBUG" /d "_X86_" /d "UNICODE"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 kernel32.lib user32.lib gdi32.lib winspool.lib comdlg32.lib advapi32.lib shell32.lib ole32.lib oleaut32.lib uuid.lib odbc32.lib odbccp32.lib /nologo /subsystem:windows /machine:I386
# ADD LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /machine:I386

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "Debug"
# PROP BASE Intermediate_Dir "Debug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "Debug"
# PROP Intermediate_Dir "Debug"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /W3 /Gm /GX /Zi /Od /D "WIN32" /D "_DEBUG" /D "_WINDOWS" /YX /FD /c
# ADD CPP /nologo /MTd /W3 /Gm /GX /ZI /Od /I "../.." /D "_DEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "_MBCS" /D "UNICODE" /YX /FD /c
# SUBTRACT CPP /X /Fr
MTL=midl.exe
# ADD BASE MTL /nologo /D "_DEBUG" /mktyplib203 /win32
# ADD MTL /nologo /D "_DEBUG" /mktyplib203 /win32
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d "_DEBUG"
# ADD RSC /l 0x409 /d "_DEBUG" /d "_X86_" /d "UNICODE"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 kernel32.lib user32.lib gdi32.lib winspool.lib comdlg32.lib advapi32.lib shell32.lib ole32.lib oleaut32.lib uuid.lib odbc32.lib odbccp32.lib /nologo /subsystem:windows /debug /machine:I386 /pdbtype:sept
# ADD LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /debug /machine:I386 /pdbtype:sept

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "punzip__"
# PROP BASE Intermediate_Dir "punzip__"
# PROP BASE Ignore_Export_Lib 0
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "AnsiRelease"
# PROP Intermediate_Dir "AnsiRelease"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /MT /W3 /GX /O2 /I "../.." /D "NDEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /YX /FD /c
# SUBTRACT BASE CPP /X /Fr
# ADD CPP /nologo /MT /W3 /GX /O2 /I "../.." /D "NDEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "_MBCS" /YX /FD /c
# SUBTRACT CPP /X /Fr
MTL=midl.exe
# ADD BASE MTL /nologo /D "NDEBUG" /mktyplib203 /win32
# ADD MTL /nologo /D "NDEBUG" /mktyplib203 /win32
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d "NDEBUG" /d "_X86_"
# ADD RSC /l 0x409 /d "NDEBUG" /d "_X86_"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /machine:I386
# ADD LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /machine:I386

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "punzip_0"
# PROP BASE Intermediate_Dir "punzip_0"
# PROP BASE Ignore_Export_Lib 0
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "AnsiDebug"
# PROP Intermediate_Dir "AnsiDebug"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /MTd /W3 /Gm /GX /Zi /Od /I "../.." /D "_DEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "UNICODE" /YX /FD /c
# SUBTRACT BASE CPP /X /Fr
# ADD CPP /nologo /MTd /W3 /Gm /GX /ZI /Od /I "../.." /D "_DEBUG" /D "_X86_" /D "_WINDOWS" /D "POCKET_UNZIP" /D "_MBCS" /YX /FD /c
# SUBTRACT CPP /X /Fr
MTL=midl.exe
# ADD BASE MTL /nologo /D "_DEBUG" /mktyplib203 /win32
# ADD MTL /nologo /D "_DEBUG" /mktyplib203 /win32
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d "_DEBUG" /d "_X86_"
# ADD RSC /l 0x409 /d "_DEBUG" /d "_X86_"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /debug /machine:I386 /pdbtype:sept
# ADD LINK32 user32.lib gdi32.lib comctl32.lib advapi32.lib shell32.lib comdlg32.lib /nologo /version:1.0 /subsystem:windows /debug /machine:I386 /pdbtype:sept

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "x86emRel"
# PROP BASE Intermediate_Dir "x86emRel"
# PROP BASE Ignore_Export_Lib 0
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "x86emRel"
# PROP Intermediate_Dir "x86emRel"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /ML /W3 /O2 /D "WIN32" /D "STRICT" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "_WIN32_WCE_EMULATION" /D "INTERNATIONAL" /D "USA" /D "INTLMSG_CODEPAGE" /D "NDEBUG" /D "x86" /D "i486" /D "_x86_" /D "POCKET_UNZIP" /D "UNICODE" /D "_MBCS" /YX /c
# ADD CPP /nologo /MT /W3 /O2 /I "../.." /I "../../wince/inc" /D "WIN32" /D "STRICT" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "_WIN32_WCE_EMULATION" /D "INTERNATIONAL" /D "USA" /D "INTLMSG_CODEPAGE" /D "NDEBUG" /D "x86" /D "i486" /D "_x86_" /D "POCKET_UNZIP" /D "UNICODE" /D "_MBCS" /YX /c
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d UNDER_CE=$(CEVersion) /d "UNICODE" /d "WIN32" /d "STRICT" /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "_WIN32_WCE_EMULATION" /d "INTERNATIONAL" /d "USA" /d "INTLMSG_CODEPAGE" /d "NDEBUG"
# ADD RSC /l 0x409 /d UNDER_CE=$(CEVersion) /d "UNICODE" /d "WIN32" /d "STRICT" /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "_WIN32_WCE_EMULATION" /d "INTERNATIONAL" /d "USA" /d "INTLMSG_CODEPAGE" /d "NDEBUG"
MTL=midl.exe
# ADD BASE MTL /nologo /D "NDEBUG" /mktyplib203 /o "NUL" /win32
# ADD MTL /nologo /D "NDEBUG" /mktyplib203 /o "NUL" /win32
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 $(CEx86Corelibc) commctrl.lib coredll.lib /nologo /stack:0x10000,0x1000 /subsystem:windows /machine:I386 /nodefaultlib:"$(CENoDefaultLib)" /windowsce:emulation
# SUBTRACT BASE LINK32 /pdb:none /nodefaultlib
# ADD LINK32 $(CEx86Corelibc) commctrl.lib coredll.lib /nologo /stack:0x10000,0x1000 /subsystem:windows /machine:I386 /nodefaultlib:"$(CENoDefaultLib)" /windowsce:emulation
# SUBTRACT LINK32 /pdb:none /nodefaultlib

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "x86emDbg"
# PROP BASE Intermediate_Dir "x86emDbg"
# PROP BASE Ignore_Export_Lib 0
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "x86emDbg"
# PROP Intermediate_Dir "x86emDbg"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
CPP=cl.exe
# ADD BASE CPP /nologo /MLd /W3 /Gm /Zi /Od /D "WIN32" /D "STRICT" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "_WIN32_WCE_EMULATION" /D "INTERNATIONAL" /D "USA" /D "INTLMSG_CODEPAGE" /D "_DEBUG" /D "x86" /D "i486" /D "_x86_" /D "POCKET_UNZIP" /D "UNICODE" /D "_MBCS" /YX /c
# SUBTRACT BASE CPP /X /u /Fr
# ADD CPP /nologo /MTd /W3 /Gm /Zi /Od /I "../.." /I "../../wince/inc" /D "WIN32" /D "STRICT" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "_WIN32_WCE_EMULATION" /D "INTERNATIONAL" /D "USA" /D "INTLMSG_CODEPAGE" /D "_DEBUG" /D "x86" /D "i486" /D "_x86_" /D "POCKET_UNZIP" /D "UNICODE" /D "_MBCS" /YX /c
# SUBTRACT CPP /X /u /Fr
RSC=rc.exe
# ADD BASE RSC /l 0x409 /d UNDER_CE=$(CEVersion) /d "UNICODE" /d "WIN32" /d "STRICT" /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "_WIN32_WCE_EMULATION" /d "INTERNATIONAL" /d "USA" /d "INTLMSG_CODEPAGE" /d "_DEBUG" /d "x86" /d "i486" /d "_x86_"
# ADD RSC /l 0x409 /d UNDER_CE=$(CEVersion) /d "UNICODE" /d "WIN32" /d "STRICT" /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "_WIN32_WCE_EMULATION" /d "INTERNATIONAL" /d "USA" /d "INTLMSG_CODEPAGE" /d "_DEBUG" /d "x86" /d "i486" /d "_x86_"
MTL=midl.exe
# ADD BASE MTL /nologo /D "_DEBUG" /mktyplib203 /o "NUL" /win32
# ADD MTL /nologo /D "_DEBUG" /mktyplib203 /o "NUL" /win32
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 $(CEx86Corelibc) commctrl.lib coredll.lib /nologo /stack:0x10000,0x1000 /subsystem:windows /debug /machine:I386 /nodefaultlib:"$(CENoDefaultLib)" /windowsce:emulation
# SUBTRACT BASE LINK32 /pdb:none /incremental:no /map /nodefaultlib
# ADD LINK32 $(CEx86Corelibc) commctrl.lib coredll.lib /nologo /stack:0x10000,0x1000 /subsystem:windows /debug /machine:I386 /nodefaultlib:"$(CENoDefaultLib)" /windowsce:emulation
# SUBTRACT LINK32 /pdb:none /incremental:no /map /nodefaultlib

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "WMIPSRel"
# PROP BASE Intermediate_Dir "WMIPSRel"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "WMIPSRel"
# PROP Intermediate_Dir "WMIPSRel"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
PFILE=pfile.exe
# ADD BASE PFILE COPY
# ADD PFILE COPY
CPP=clmips.exe
# ADD BASE CPP /nologo /M$(CECrt) /W3 /O2 /D "NDEBUG" /D "MIPS" /D "_MIPS_" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "UNICODE" /YX /QMRWCE /c
# ADD CPP /nologo /M$(CECrtMT) /W3 /O2 /I "../.." /I "../../wince/inc" /D "NDEBUG" /D "MIPS" /D "_MIPS_" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "POCKET_UNZIP" /D "UNICODE" /D "_MBCS" /YX /QMRWCE /c
# SUBTRACT CPP /X /Fr
RSC=rc.exe
# ADD BASE RSC /l 0x409 /r /d "MIPS" /d "_MIPS_" /d UNDER_CE=$(CEVersion) /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "UNICODE" /d "NDEBUG"
# ADD RSC /l 0x409 /r /d "MIPS" /d "_MIPS_" /d UNDER_CE=$(CEVersion) /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "UNICODE" /d "NDEBUG"
MTL=midl.exe
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 commctrl.lib coredll.lib libc.lib /nologo /machine:MIPS /subsystem:windowsce
# SUBTRACT BASE LINK32 /pdb:none /nodefaultlib
# ADD LINK32 coredll.lib commctrl.lib /nologo /machine:MIPS /nodefaultlib:"$(CENoDefaultLib)" /subsystem:windowsce
# SUBTRACT LINK32 /pdb:none /nodefaultlib

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "WMIPSDbg"
# PROP BASE Intermediate_Dir "WMIPSDbg"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "WMIPSDbg"
# PROP Intermediate_Dir "WMIPSDbg"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
PFILE=pfile.exe
# ADD BASE PFILE COPY
# ADD PFILE COPY
CPP=clmips.exe
# ADD BASE CPP /nologo /M$(CECrtDebug) /W3 /Zi /Od /D "DEBUG" /D "MIPS" /D "_MIPS_" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "UNICODE" /YX /QMRWCE /c
# ADD CPP /nologo /M$(CECrtMTDebug) /W3 /Zi /Od /I "../.." /I "../../wince/inc" /D "DEBUG" /D "MIPS" /D "_MIPS_" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "POCKET_UNZIP" /D "UNICODE" /D "_MBCS" /YX /QMRWCE /c
# SUBTRACT CPP /X /Fr
RSC=rc.exe
# ADD BASE RSC /l 0x409 /r /d "MIPS" /d "_MIPS_" /d UNDER_CE=$(CEVersion) /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "UNICODE" /d "DEBUG"
# ADD RSC /l 0x409 /r /d "MIPS" /d "_MIPS_" /d UNDER_CE=$(CEVersion) /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "UNICODE" /d "DEBUG"
MTL=midl.exe
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 commctrl.lib coredll.lib libcd.lib /nologo /debug /machine:MIPS /subsystem:windowsce
# SUBTRACT BASE LINK32 /pdb:none /nodefaultlib
# ADD LINK32 coredll.lib commctrl.lib /nologo /debug /machine:MIPS /nodefaultlib:"$(CENoDefaultLib)" /subsystem:windowsce
# SUBTRACT LINK32 /pdb:none /nodefaultlib

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "WSH3Rel"
# PROP BASE Intermediate_Dir "WSH3Rel"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "WSH3Rel"
# PROP Intermediate_Dir "WSH3Rel"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
PFILE=pfile.exe
# ADD BASE PFILE COPY
# ADD PFILE COPY
CPP=shcl.exe
# ADD BASE CPP /nologo /M$(CECrt) /W3 /O2 /D "NDEBUG" /D "SHx" /D "SH3" /D "_SH3_" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "UNICODE" /YX /c
# ADD CPP /nologo /M$(CECrtMT) /W3 /O2 /I "../.." /I "../../wince/inc" /D "NDEBUG" /D "SHx" /D "SH3" /D "_SH3_" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "POCKET_UNZIP" /D "UNICODE" /D "_MBCS" /YX /c
# SUBTRACT CPP /X /Fr
RSC=rc.exe
# ADD BASE RSC /l 0x409 /r /d "SHx" /d "SH3" /d "_SH3_" /d UNDER_CE=$(CEVersion) /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "UNICODE" /d "NDEBUG"
# ADD RSC /l 0x409 /r /d "SH3" /d "SHx" /d "_SH3_" /d UNDER_CE=$(CEVersion) /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "UNICODE" /d "NDEBUG"
MTL=midl.exe
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 commctrl.lib coredll.lib libc.lib /nologo /machine:SH3 /subsystem:windowsce
# SUBTRACT BASE LINK32 /pdb:none /nodefaultlib
# ADD LINK32 coredll.lib commctrl.lib /nologo /machine:SH3 /nodefaultlib:"$(CENoDefaultLib)" /subsystem:windowsce
# SUBTRACT LINK32 /pdb:none /nodefaultlib

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "WSH3Dbg"
# PROP BASE Intermediate_Dir "WSH3Dbg"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "WSH3Dbg"
# PROP Intermediate_Dir "WSH3Dbg"
# PROP Ignore_Export_Lib 0
# PROP Target_Dir ""
PFILE=pfile.exe
# ADD BASE PFILE COPY
# ADD PFILE COPY
CPP=shcl.exe
# ADD BASE CPP /nologo /M$(CECrtDebug) /W3 /Zi /Od /D "DEBUG" /D "SHx" /D "SH3" /D "_SH3_" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "UNICODE" /YX /c
# ADD CPP /nologo /M$(CECrtMTDebug) /W3 /Zi /Od /I "../.." /I "../../wince/inc" /D "DEBUG" /D "SHx" /D "SH3" /D "_SH3_" /D UNDER_CE=$(CEVersion) /D _WIN32_WCE=$(CEVersion) /D "$(CEConfigName)" /D "POCKET_UNZIP" /D "UNICODE" /D "_MBCS" /YX /c
# SUBTRACT CPP /X /u /Fr
RSC=rc.exe
# ADD BASE RSC /l 0x409 /r /d "SHx" /d "SH3" /d "_SH3_" /d UNDER_CE=$(CEVersion) /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "UNICODE" /d "DEBUG"
# ADD RSC /l 0x409 /r /d "SHx" /d "SH3" /d "_SH3_" /d UNDER_CE=$(CEVersion) /d _WIN32_WCE=$(CEVersion) /d "$(CEConfigName)" /d "UNICODE" /d "DEBUG"
MTL=midl.exe
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
# ADD BSC32 /nologo
LINK32=link.exe
# ADD BASE LINK32 commctrl.lib coredll.lib libcd.lib /nologo /debug /machine:SH3 /subsystem:windowsce
# SUBTRACT BASE LINK32 /pdb:none /nodefaultlib
# ADD LINK32 coredll.lib commctrl.lib /nologo /debug /machine:SH3 /nodefaultlib:"$(CENoDefaultLib)" /subsystem:windowsce
# SUBTRACT LINK32 /pdb:none /map /nodefaultlib

!ENDIF 

# Begin Target

# Name "punzip - Win32 Release"
# Name "punzip - Win32 Debug"
# Name "punzip - Win32 ANSI Release"
# Name "punzip - Win32 ANSI Debug"
# Name "punzip - Win32 (WCE x86em) Release"
# Name "punzip - Win32 (WCE x86em) Debug"
# Name "punzip - Win32 (WCE MIPS) Release"
# Name "punzip - Win32 (WCE MIPS) Debug"
# Name "punzip - Win32 (WCE SH3) Release"
# Name "punzip - Win32 (WCE SH3) Debug"
# Begin Group "Info-ZIP Sources"

# PROP Default_Filter ""
# Begin Source File

SOURCE=..\..\api.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\acorn\swiven.h"\
	"..\..\amiga\amiga.h"\
	"..\..\amiga\memwatch.h"\
	"..\..\amiga\z-stat.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\stat.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_API_C=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_API_C=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\consts.h
# End Source File
# Begin Source File

SOURCE=..\..\crc32.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_CRC32=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRC32=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\crc32.h
# End Source File
# Begin Source File

SOURCE=..\..\crypt.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_CRYPT=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_CRYPT=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\crypt.h
# End Source File
# Begin Source File

SOURCE=..\..\ebcdic.h
# End Source File
# Begin Source File

SOURCE=..\..\explode.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_EXPLO=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXPLO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\extract.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_EXTRA=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_EXTRA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\fileio.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\charconv.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\charconv.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\charconv.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_FILEI=\
	"..\..\crc32.h"\
	"..\..\crypt.h"\
	"..\..\ebcdic.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_FILEI=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\globals.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_GLOBA=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_GLOBA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\globals.h
# End Source File
# Begin Source File

SOURCE=..\..\inflate.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_INFLA=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\inflate.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_INFLA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\inflate.h
# End Source File
# Begin Source File

SOURCE=..\..\list.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_LIST_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_LIST_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\process.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_PROCE=\
	"..\..\crc32.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_PROCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\ttyio.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_TTYIO=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\ttyio.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\zip.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_TTYIO=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\ttyio.h
# End Source File
# Begin Source File

SOURCE=..\..\ubz2err.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_UBZ2ERR_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_UBZ2ERR_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\..\windll\windll.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_UBZ2ERR_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_UBZ2ERR_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_UBZ2ERR_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\decs.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_UBZ2ERR_=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UBZ2ERR_=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\windll.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\unreduce.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_UNRED=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNRED=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\unshrink.c

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_UNSHR=\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_UNSHR=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\..\unzip.h
# End Source File
# Begin Source File

SOURCE=..\..\unzpriv.h
# End Source File
# Begin Source File

SOURCE=..\..\unzvers.h
# End Source File
# Begin Source File

SOURCE=..\..\zip.h
# End Source File
# End Group
# Begin Group "Pocket UnZip Sources"

# PROP Default_Filter ""
# Begin Source File

SOURCE=..\intrface.cpp

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_INTRF=\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_INTRF=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\intrface.h
# End Source File
# Begin Source File

SOURCE=..\punzip.h
# End Source File
# Begin Source File

SOURCE=..\punzip.rc

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\punzip.rcv
# End Source File
# Begin Source File

SOURCE=..\resource.h
# End Source File
# Begin Source File

SOURCE=..\wcecfg.h
# End Source File
# Begin Source File

SOURCE=..\wince.cpp

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_WINCE=\
	"..\..\globals.h"\
	"..\..\timezone.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_WINCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_WINCE=\
	"..\..\globals.h"\
	"..\..\timezone.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_WINCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_WINCE=\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_WINCE=\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_WINCE=\
	"..\..\globals.h"\
	"..\..\timezone.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	
NODEP_CPP_WINCE=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_WINCE=\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\wince.h
# End Source File
# Begin Source File

SOURCE=..\winmain.cpp

!IF  "$(CFG)" == "punzip - Win32 Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Release"

!ELSEIF  "$(CFG)" == "punzip - Win32 ANSI Debug"

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Release"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE x86em) Debug"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\win32\rsxntwin.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Release"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE MIPS) Debug"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Release"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\..\windll\structs.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\flexos\flxcfg.h"\
	"..\..\maccfg.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\netware\nlmcfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\tandem.h"\
	"..\..\theos\thscfg.h"\
	"..\..\unix\unxcfg.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ELSEIF  "$(CFG)" == "punzip - Win32 (WCE SH3) Debug"

DEP_CPP_WINMA=\
	"..\..\consts.h"\
	"..\..\crypt.h"\
	"..\..\globals.h"\
	"..\..\unzip.h"\
	"..\..\unzpriv.h"\
	"..\..\unzvers.h"\
	"..\intrface.h"\
	"..\punzip.h"\
	"..\punzip.rcv"\
	"..\wcecfg.h"\
	"..\wince.h"\
	"..\winmain.h"\
	
NODEP_CPP_WINMA=\
	"..\..\acorn\riscos.h"\
	"..\..\amiga\amiga.h"\
	"..\..\aosvs\aosvs.h"\
	"..\..\macdir.h"\
	"..\..\macstat.h"\
	"..\..\msdos\doscfg.h"\
	"..\..\os2\os2cfg.h"\
	"..\..\os2\os2data.h"\
	"..\..\qdos\izqdos.h"\
	"..\..\vmmvs.h"\
	"..\..\win32\w32cfg.h"\
	"..\..\zlib.h"\
	

!ENDIF 

# End Source File
# Begin Source File

SOURCE=..\winmain.h
# End Source File
# End Group
# Begin Group "Resources"

# PROP Default_Filter ""
# Begin Source File

SOURCE=..\ilmask.bmp
# End Source File
# Begin Source File

SOURCE=..\imglist.2bp
# End Source File
# Begin Source File

SOURCE=..\imglist.bmp
# End Source File
# Begin Source File

SOURCE=..\punzip.ic2
# End Source File
# Begin Source File

SOURCE=..\punzip.ico
# End Source File
# Begin Source File

SOURCE=..\toolbar.2bp
# End Source File
# Begin Source File

SOURCE=..\toolbar.bmp
# End Source File
# Begin Source File

SOURCE=..\zipfile.ic2
# End Source File
# Begin Source File

SOURCE=..\zipfile.ico
# End Source File
# End Group
# Begin Group "Documentation"

# PROP Default_Filter ""
# Begin Source File

SOURCE=..\Contents
# End Source File
# Begin Source File

SOURCE=..\punzip.htp
# End Source File
# Begin Source File

SOURCE=..\README
# End Source File
# End Group
# End Target
# End Project
