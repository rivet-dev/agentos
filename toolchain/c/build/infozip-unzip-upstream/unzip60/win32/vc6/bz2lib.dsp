# Microsoft Developer Studio Project File - Name="bz2lib" - Package Owner=<4>
# Microsoft Developer Studio Generated Build File, Format Version 6.00
# ** NICHT BEARBEITEN **

# TARGTYPE "Win32 (x86) Static Library" 0x0104

CFG=bz2lib - Win32 Debug
!MESSAGE Dies ist kein gültiges Makefile. Zum Erstellen dieses Projekts mit NMAKE
!MESSAGE verwenden Sie den Befehl "Makefile exportieren" und führen Sie den Befehl
!MESSAGE 
!MESSAGE NMAKE /f "bz2lib.mak".
!MESSAGE 
!MESSAGE Sie können beim Ausführen von NMAKE eine Konfiguration angeben
!MESSAGE durch Definieren des Makros CFG in der Befehlszeile. Zum Beispiel:
!MESSAGE 
!MESSAGE NMAKE /f "bz2lib.mak" CFG="bz2lib - Win32 ASM Debug"
!MESSAGE 
!MESSAGE Für die Konfiguration stehen zur Auswahl:
!MESSAGE 
!MESSAGE "bz2lib - Win32 Release" (basierend auf  "Win32 (x86) Static Library")
!MESSAGE "bz2lib - Win32 Debug" (basierend auf  "Win32 (x86) Static Library")
!MESSAGE "bz2lib - Win32 ASM Release" (basierend auf  "Win32 (x86) Static Library")
!MESSAGE "bz2lib - Win32 ASM Debug" (basierend auf  "Win32 (x86) Static Library")
!MESSAGE 

# Begin Project
# PROP AllowPerConfigDependencies 0
# PROP Scc_ProjName ""
# PROP Scc_LocalPath ""
CPP=cl.exe
RSC=rc.exe

!IF  "$(CFG)" == "bz2lib - Win32 Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "bz2lib__Win32_Release"
# PROP BASE Intermediate_Dir "bz2lib__Win32_Release"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "bz2lib__Win32_Release"
# PROP Intermediate_Dir "bz2lib__Win32_Release"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /W3 /GX /O2 /D "WIN32" /D "NDEBUG" /D "_MBCS" /D "_LIB" /YX /FD /c
# ADD CPP /nologo /W3 /GX /O2 /Oy- /D "NDEBUG" /D "WIN32" /D "_MBCS" /D "_LIB" /D "BZ_NO_STDIO" /FD /c
# SUBTRACT CPP /YX /Yc /Yu
# ADD BASE RSC /l 0x407 /d "NDEBUG"
# ADD RSC /l 0x409 /d "NDEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo /out:"bz2lib__Win32_Release\bz2.lib"

!ELSEIF  "$(CFG)" == "bz2lib - Win32 Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "bz2lib__Win32_Debug"
# PROP BASE Intermediate_Dir "bz2lib__Win32_Debug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "bz2lib__Win32_Debug"
# PROP Intermediate_Dir "bz2lib__Win32_Debug"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /W3 /Gm /GX /ZI /Od /D "WIN32" /D "_DEBUG" /D "_MBCS" /D "_LIB" /YX /FD /GZ /c
# ADD CPP /nologo /W3 /Gm /GX /ZI /Od /D "_DEBUG" /D "WIN32" /D "_MBCS" /D "_LIB" /D "BZ_NO_STDIO" /FD /GZ /c
# SUBTRACT CPP /YX /Yc /Yu
# ADD BASE RSC /l 0x407 /d "_DEBUG"
# ADD RSC /l 0x409 /d "_DEBUG"
BSC32=bscmake.exe
# ADD BASE BSC32 /nologo
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo
# ADD LIB32 /nologo /out:"bz2lib__Win32_Debug\bz2.lib"

!ELSEIF  "$(CFG)" == "bz2lib - Win32 ASM Release"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 0
# PROP BASE Output_Dir "bz2lib___Win32_ASM_Release"
# PROP BASE Intermediate_Dir "bz2lib___Win32_ASM_Release"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 0
# PROP Output_Dir "bz2lib__Win32_Release"
# PROP Intermediate_Dir "bz2lib__Win32_Release"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /W3 /GX /O2 /Oy- /D "NDEBUG" /D "WIN32" /D "_MBCS" /D "_LIB" /D "BZ_NO_STDIO" /FD /c
# SUBTRACT BASE CPP /YX /Yc /Yu
# ADD CPP /nologo /W3 /GX /O2 /Oy- /D "NDEBUG" /D "WIN32" /D "_MBCS" /D "_LIB" /D "BZ_NO_STDIO" /FD /c
# SUBTRACT CPP /YX /Yc /Yu
# ADD BASE RSC /l 0x409 /d "NDEBUG"
# ADD RSC /l 0x409 /d "NDEBUG"
BSC32=bscmake.exe
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo /out:"bz2lib__Win32_Release\bz2.lib"
# ADD LIB32 /nologo /out:"bz2lib__Win32_Release\bz2.lib"

!ELSEIF  "$(CFG)" == "bz2lib - Win32 ASM Debug"

# PROP BASE Use_MFC 0
# PROP BASE Use_Debug_Libraries 1
# PROP BASE Output_Dir "bz2lib___Win32_ASM_Debug"
# PROP BASE Intermediate_Dir "bz2lib___Win32_ASM_Debug"
# PROP BASE Target_Dir ""
# PROP Use_MFC 0
# PROP Use_Debug_Libraries 1
# PROP Output_Dir "bz2lib__Win32_Debug"
# PROP Intermediate_Dir "bz2lib__Win32_Debug"
# PROP Target_Dir ""
# ADD BASE CPP /nologo /W3 /Gm /GX /ZI /Od /D "_DEBUG" /D "WIN32" /D "_MBCS" /D "_LIB" /D "BZ_NO_STDIO" /FD /GZ /c
# SUBTRACT BASE CPP /YX /Yc /Yu
# ADD CPP /nologo /W3 /Gm /GX /ZI /Od /D "_DEBUG" /D "WIN32" /D "_MBCS" /D "_LIB" /D "BZ_NO_STDIO" /FD /GZ /c
# SUBTRACT CPP /YX /Yc /Yu
# ADD BASE RSC /l 0x409 /d "_DEBUG"
# ADD RSC /l 0x409 /d "_DEBUG"
BSC32=bscmake.exe
LIB32=link.exe -lib
# ADD BASE LIB32 /nologo /out:"bz2lib__Win32_Debug\bz2.lib"
# ADD LIB32 /nologo /out:"bz2lib__Win32_Debug\bz2.lib"

!ENDIF 

# Begin Target

# Name "bz2lib - Win32 Release"
# Name "bz2lib - Win32 Debug"
# Name "bz2lib - Win32 ASM Release"
# Name "bz2lib - Win32 ASM Debug"
# Begin Group "Source Files"

# PROP Default_Filter "cpp;c;cxx;rc;def;r;odl;idl;hpj;bat"
# Begin Source File

SOURCE=..\..\bzip2\blocksort.c
# End Source File
# Begin Source File

SOURCE=..\..\bzip2\bzlib.c
# End Source File
# Begin Source File

SOURCE=..\..\bzip2\compress.c
# End Source File
# Begin Source File

SOURCE=..\..\bzip2\crctable.c
# End Source File
# Begin Source File

SOURCE=..\..\bzip2\decompress.c
# End Source File
# Begin Source File

SOURCE=..\..\bzip2\huffman.c
# End Source File
# Begin Source File

SOURCE=..\..\bzip2\randtable.c
# End Source File
# End Group
# Begin Group "Header Files"

# PROP Default_Filter "h;hpp;hxx;hm;inl"
# Begin Source File

SOURCE=..\..\bzip2\bzlib.h
# End Source File
# Begin Source File

SOURCE=..\..\bzip2\bzlib_private.h
# End Source File
# End Group
# End Target
# End Project
