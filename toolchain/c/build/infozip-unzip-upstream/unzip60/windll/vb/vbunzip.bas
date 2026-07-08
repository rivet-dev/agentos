Attribute VB_Name = "VBUnzBas"
Option Explicit

'-- Please Do Not Remove These Comment Lines!
'----------------------------------------------------------------
'-- Sample VB 5 / VB 6 code to drive unzip32.dll
'-- Contributed to the Info-ZIP project by Mike Le Voi
'--
'-- Contact me at: mlevoi@modemss.brisnet.org.au
'--
'-- Visit my home page at: http://modemss.brisnet.org.au/~mlevoi
'--
'-- Use this code at your own risk. Nothing implied or warranted
'-- to work on your machine :-)
'----------------------------------------------------------------
'--
'-- This Source Code Is Freely Available From The Info-ZIP Project
'-- Web Server At:
'-- ftp://ftp.info-zip.org/pub/infozip/infozip.html
'--
'-- A Very Special Thanks To Mr. Mike Le Voi
'-- And Mr. Mike White
'-- And The Fine People Of The Info-ZIP Group
'-- For Letting Me Use And Modify Their Original
'-- Visual Basic 5.0 Code! Thank You Mike Le Voi.
'-- For Your Hard Work In Helping Me Get This To Work!!!
'---------------------------------------------------------------
'--
'-- Contributed To The Info-ZIP Project By Raymond L. King.
'-- Modified June 21, 1998
'-- By Raymond L. King
'-- Custom Software Designers
'--
'-- Contact Me At: king@ntplx.net
'-- ICQ 434355
'-- Or Visit Our Home Page At: http://www.ntplx.net/~king
'--
'---------------------------------------------------------------
'--
'-- Modified August 17, 1998
'--  by Christian Spieler
'--  (implemented sort of a "real" user interface)
'-- Modified May 11, 2003
'--  by Christian Spieler
'--  (use late binding for referencing the common dialog)
'-- Modified February 01, 2008
'--  by Christian Spieler
'--  (adapted DLL interface changes, fixed UZDLLPass callback)
'-- Modified December 08, 2008 to December 30, 2008
'--  by Ed Gordon
'--  Updated sample project for UnZip 6.0 unzip32.dll
'--  (support UnZip 6.0 flags and structures)
'-- Modified January 03, 2009
'--  by Christian Spieler
'--  (better solution for overwrite_all handling, use Double
'--  instead of Currency to stay safe against number overflow,
'--  corrected UZDLLServ_I32() calling interface,
'--  removed code that is unsupported under VB5)
'--
'---------------------------------------------------------------

'-- Expected Version data for the DLL compatibility check
'
'   For consistency of the version checking algorithm, the version number
'   constants "UzDLL_MinVer" and "UzDLL_MaxAPI" have to fullfil the
'   condition "UzDLL_MinVer <= "UzDLL_MaxAPI".
'   Version data supplied by a specific UnZip DLL always obey the
'   relation  "UzDLL Version" >= "UzDLL API".

'Oldest UnZip DLL version that is supported by this program
Private Const cUzDLL_MinVer_Major As Byte = 6
Private Const cUzDLL_MinVer_Minor As Byte = 0
Private Const cUzDLL_MinVer_Revis As Byte = 0

'Last (newest) UnZip DLL API version that is known (and supported)
'by this program
Private Const cUzDLL_MaxAPI_Major As Byte = 6
Private Const cUzDLL_MaxAPI_Minor As Byte = 0
Private Const cUzDLL_MaxAPI_Revis As Byte = 0

'Current structure version ID of the DCLIST structure layout
Private Const cUz_DCLStructVer As Long = &H600

'-- C Style argv
Private Type UNZIPnames
  uzFiles(0 To 99) As String
End Type

'-- Callback Large "String"
Private Type UNZIPCBChar
  ch(32800) As Byte
End Type

'-- Callback Small "String"
Private Type UNZIPCBCh
  ch(256) As Byte
End Type

'-- UNZIP32.DLL DCL Structure
Private Type DCLIST
  StructVersID      As Long    ' Currently version &H600 of this structure
  ExtractOnlyNewer  As Long    ' 1 = Extract Only Newer/New, Else 0
  SpaceToUnderscore As Long    ' 1 = Convert Space To Underscore, Else 0
  PromptToOverwrite As Long    ' 1 = Prompt To Overwrite Required, Else 0
  fQuiet            As Long    ' 2 = No Messages, 1 = Less, 0 = All
  ncflag            As Long    ' 1 = Write To Stdout, Else 0
  ntflag            As Long    ' 1 = Test Zip File, Else 0
  nvflag            As Long    ' 0 = Extract, 1 = List Zip Contents
  nfflag            As Long    ' 1 = Extract Only Newer Over Existing, Else 0
  nzflag            As Long    ' 1 = Display Zip File Comment, Else 0
  ndflag            As Long    ' 0 = Junk paths, 1 = safe path components only, 2 = all
  noflag            As Long    ' 1 = Overwrite Files, Else 0
  naflag            As Long    ' 1 = Convert CR To CRLF, Else 0
  nZIflag           As Long    ' 1 = Zip Info Verbose, Else 0
  B_flag            As Long    ' 1 = Backup existing files, Else 0
  C_flag            As Long    ' 1 = Case Insensitivity, 0 = Case Sensitivity
  D_flag            As Long    ' Timestamp restoration, 0 = All, 1 = Files, 2 = None
  U_flag            As Long    ' 0 = Unicode enabled, 1 = Escape chars, 2 = No Unicode
  fPrivilege        As Long    ' 1 = ACL, 2 = Privileges
  Zip               As String  ' The Zip Filename To Extract Files
  ExtractDir        As String  ' The Extraction Directory, NULL If Extracting To Current Dir
End Type

'-- UNZIP32.DLL Userfunctions Structure
Private Type USERFUNCTION
  UZDLLPrnt         As Long     ' Pointer To Apps Print Function
  UZDLLSND          As Long     ' Pointer To Apps Sound Function
  UZDLLREPLACE      As Long     ' Pointer To Apps Replace Function
  UZDLLPASSWORD     As Long     ' Pointer To Apps Password Function
  ' 64-bit versions (VB6 does not support passing 64-bit values!)
  UZDLLMESSAGE      As Long     ' Pointer To Apps Message Function (Not Used!)
  UZDLLSERVICE      As Long     ' Pointer To Apps Service Function (Not Used!)
  ' 32-bit versions
  UZDLLMESSAGE_I32  As Long     ' Pointer To Apps Message Function
  UZDLLSERVICE_I32  As Long     ' Pointer To Apps Service Function
  ' All 64-bit values passed as low and high parts!
  TotalSizeComp_Lo  As Long     ' Total Size Of Zip Archive (low 32 bits)
  TotalSizeComp_Hi  As Long     ' Total Size Of Zip Archive (high 32 bits)
  TotalSize_Lo      As Long     ' Total Size Of All Files In Archive (low 32)
  TotalSize_Hi      As Long     ' Total Size Of All Files In Archive (high 32)
  NumMembers_Lo     As Long     ' Total Number Of All Files In The Archive (low 32)
  NumMembers_Hi     As Long     ' Total Number Of All Files In The Archive (high 32)
  CompFactor        As Long     ' Compression Factor
  cchComment        As Integer  ' Flag If Archive Has A Comment!
End Type

'-- UNZIP32.DLL Version Structure
Private Type UZPVER2
  structlen       As Long         ' Length Of The Structure Being Passed
  flag            As Long         ' Bit 0: is_beta  bit 1: uses_zlib
  beta            As String * 10  ' e.g., "g BETA" or ""
  date            As String * 20  ' e.g., "4 Sep 95" (beta) or "4 September 1995"
  zlib            As String * 10  ' e.g., "1.0.5" or NULL
  unzip(1 To 4)   As Byte         ' Version Type Unzip
  zipinfo(1 To 4) As Byte         ' Version Type Zip Info
  os2dll          As Long         ' Version Type OS2 DLL
  windll(1 To 4)  As Byte         ' Version Type Windows DLL
  dllapimin(1 To 4) As Byte       ' Version Type DLL API minimum compatibility
End Type

'-- This assumes UNZIP32.DLL is somewhere on your execution path!
'-- The term "execution path" means a search in the following locations,
'-- in the listed sequence (for more details look up the documentation
'-- of the LoadLibrary() Win32 API call):
'--  1) the directory from which the VB6 application was loaded,
'--  2) your current working directory in effect when the VB6 program
'--     tries to access a first API call of UNZIP32.DLL,
'--  3) the Windows "SYSTEM32" (only NT/2K/XP...) and "SYSTEM" directories,
'--     and the Windows directory,
'--  4) the folder list of your command path (e.g. check the environment
'--     variable PATH as set in a console window started from scratch).
'-- Normally, the Windows system directory is on your command path,
'-- so installing the UNZIP32.DLL in the Windows System Directory
'-- should always work.
'--
'-- WARNING:
'-- When a VB6 program is run in the VB6 IDE, the "directory from which the
'-- application was loaded" is the
'--  ===>>> directory where VB6.EXE is stored (!!!),
'-- not the storage directory of the VB project file
'-- (the folder returned by "App.Path").
'-- When a compiled VB6 program is run, the "application load directory"
'-- is identical with the folder reported by "App.Path".
'--
Private Declare Function Wiz_SingleEntryUnzip Lib "unzip32.dll" _
  (ByVal ifnc As Long, ByRef ifnv As UNZIPnames, _
   ByVal xfnc As Long, ByRef xfnv As UNZIPnames, _
   dcll As DCLIST, Userf As USERFUNCTION) As Long

Private Declare Function UzpVersion2 Lib "unzip32.dll" _
  (uzpv As UZPVER2) As Long

'-- Private variable holding the API version id as reported by the
'-- loaded UnZip DLL
Private m_UzDllApiVers As Long

'-- Private Variables For Structure Access
Private UZDCL  As DCLIST
Private UZUSER As USERFUNCTION
Private UZVER2 As UZPVER2

'-- Public Variables For Setting The
'-- UNZIP32.DLL DCLIST Structure
'-- These Must Be Set Before The Actual Call To VBUnZip32
Public uExtractOnlyNewer As Long     ' 1 = Extract Only Newer/New, Else 0
Public uSpaceUnderScore  As Long     ' 1 = Convert Space To Underscore, Else 0
Public uPromptOverWrite  As Long     ' 1 = Prompt To Overwrite Required, Else 0
Public uQuiet            As Long     ' 2 = No Messages, 1 = Less, 0 = All
Public uWriteStdOut      As Long     ' 1 = Write To Stdout, Else 0
Public uTestZip          As Long     ' 1 = Test Zip File, Else 0
Public uExtractList      As Long     ' 0 = Extract, 1 = List Contents
Public uFreshenExisting  As Long     ' 1 = Update Existing by Newer, Else 0
Public uDisplayComment   As Long     ' 1 = Display Zip File Comment, Else 0
Public uHonorDirectories As Long     ' 1 = Honor Directories, Else 0
Public uOverWriteFiles   As Long     ' 1 = Overwrite Files, Else 0
Public uConvertCR_CRLF   As Long     ' 1 = Convert CR To CRLF, Else 0
Public uVerbose          As Long     ' 1 = Zip Info Verbose
Public uCaseSensitivity  As Long     ' 1 = Case Insensitivity, 0 = Case Sensitivity
Public uPrivilege        As Long     ' 1 = ACL, 2 = Privileges, Else 0
Public uZipFileName      As String   ' The Zip File Name
Public uExtractDir       As String   ' Extraction Directory, Null If Current Directory

'-- Public Program Variables
Public uZipNumber    As Long         ' Zip File Number
Public uNumberFiles  As Long         ' Number Of Files
Public uNumberXFiles As Long         ' Number Of Extracted Files
Public uZipMessage   As String       ' For Zip Message
Public uZipInfo      As String       ' For Zip Information
Public uZipNames     As UNZIPnames   ' Names Of Files To Unzip
Public uExcludeNames As UNZIPnames   ' Names Of Zip Files To Exclude
Public uVbSkip       As Boolean      ' For DLL Password Function

'-- Puts A Function Pointer In A Structure
'-- For Callbacks.
Public Function FnPtr(ByVal lp As Long) As Long

  FnPtr = lp

End Function

'-- Callback For UNZIP32.DLL - Receive Message Function
Public Sub UZReceiveDLLMessage_I32( _
    ByVal ucsize_lo As Long, _
    ByVal ucsize_hi As Long, _
    ByVal csiz_lo As Long, _
    ByVal csiz_hi As Long, _
    ByVal cfactor As Integer, _
    ByVal mo As Integer, _
    ByVal dy As Integer, _
    ByVal yr As Integer, _
    ByVal hh As Integer, _
    ByVal mm As Integer, _
    ByVal c As Byte, _
    ByRef fname As UNZIPCBCh, _
    ByRef meth As UNZIPCBCh, _
    ByVal crc As Long, _
    ByVal fCrypt As Byte)

  Dim s0     As String
  Dim xx     As Long
  Dim cCh    As Byte
  Dim strout As String * 80
  Dim ucsize As Double
  Dim csiz   As Double

  '-- Always implement a runtime error handler in Callback Routines!
  On Error Resume Next

  '------------------------------------------------
  '-- This Is Where The Received Messages Are
  '-- Printed Out And Displayed.
  '-- You Can Modify Below!
  '------------------------------------------------

  strout = Space$(80)

  '-- For Zip Message Printing
  If uZipNumber = 0 Then
    Mid$(strout, 1, 50) = "Filename:"
    Mid$(strout, 53, 4) = "Size"
    Mid$(strout, 62, 4) = "Date"
    Mid$(strout, 71, 4) = "Time"
    uZipMessage = strout & vbNewLine
    strout = Space$(80)
  End If

  s0 = ""

  '-- Do Not Change This For Next!!!
  For xx = 0 To UBound(fname.ch)
    If fname.ch(xx) = 0 Then Exit For
    s0 = s0 & Chr$(fname.ch(xx))
  Next

  ucsize = CnvI64Struct2Dbl(ucsize_lo, ucsize_hi)
  csiz = CnvI64Struct2Dbl(csiz_lo, csiz_hi)

  '-- Assign Zip Information For Printing
  Mid$(strout, 1, 50) = Mid$(s0, 1, 50)
  Mid$(strout, 51, 9) = Right$("        " & CStr(ucsize), 9)
  Mid$(strout, 62, 3) = Right$("0" & Trim$(CStr(mo)), 2) & "/"
  Mid$(strout, 65, 3) = Right$("0" & Trim$(CStr(dy)), 2) & "/"
  Mid$(strout, 68, 2) = Right$("0" & Trim$(CStr(yr)), 2)
  Mid$(strout, 72, 3) = Right$(Str$(hh), 2) & ":"
  Mid$(strout, 75, 2) = Right$("0" & Trim$(CStr(mm)), 2)

  ' Mid$(strout, 77, 2) = Right$(" " & CStr(cfactor), 2)
  ' Mid$(strout, 80, 8) = Right$("        " & CStr(csiz), 8)
  ' s0 = ""
  ' For xx = 0 To 255
  '     If meth.ch(xx) = 0 Then Exit For
  '     s0 = s0 & Chr$(meth.ch(xx))
  ' Next xx

  '-- Do Not Modify Below!!!
  uZipMessage = uZipMessage & strout & vbNewLine
  uZipNumber = uZipNumber + 1

End Sub

'-- Callback For UNZIP32.DLL - Print Message Function
Public Function UZDLLPrnt(ByRef fname As UNZIPCBChar, ByVal x As Long) As Long

  Dim s0 As String
  Dim xx As Long
  Dim cCh As Byte

  '-- Always implement a runtime error handler in Callback Routines!
  On Error Resume Next

  s0 = ""

  '-- Gets The UNZIP32.DLL Message For Displaying.
  For xx = 0 To x - 1
    cCh = fname.ch(xx)
    Select Case cCh
    Case 0
      Exit For
    Case 10
      s0 = s0 & vbNewLine     ' Damn UNIX :-)
    Case 92 ' = Asc("\")
      s0 = s0 & "/"
    Case Else
      s0 = s0 & Chr$(cCh)
    End Select
  Next

  '-- Assign Zip Information
  uZipInfo = uZipInfo & s0

  UZDLLPrnt = 0

End Function

'-- Callback For UNZIP32.DLL - DLL Service Function
Public Function UZDLLServ_I32(ByRef mname As UNZIPCBChar, _
         ByVal lUcSiz_Lo As Long, ByVal lUcSiz_Hi As Long) As Long

  Dim UcSiz As Double
  Dim s0 As String
  Dim xx As Long

  '-- Always implement a runtime error handler in Callback Routines!
  On Error Resume Next

  ' Parameters lUcSiz_Lo and lUcSiz_Hi contain the uncompressed size
  ' of the extracted archive entry.
  ' This information may be used for some kind of progress display...
  UcSiz = CnvI64Struct2Dbl(lUcSiz_Lo, lUcSiz_Hi)

  s0 = ""
  '-- Get Zip32.DLL Message For processing
  For xx = 0 To UBound(mname.ch)
    If mname.ch(xx) = 0 Then Exit For
    s0 = s0 & Chr$(mname.ch(xx))
  Next
  ' At this point, s0 contains the message passed from the DLL
  ' (like the current file being extracted)
  ' It is up to the developer to code something useful here :)

  UZDLLServ_I32 = 0 ' Setting this to 1 will abort the zip!

End Function

'-- Callback For UNZIP32.DLL - Password Function
Public Function UZDLLPass(ByRef pwbuf As UNZIPCBCh, _
  ByVal bufsiz As Long, ByRef promptmsg As UNZIPCBCh, _
  ByRef entryname As UNZIPCBCh) As Long

  Dim prompt     As String
  Dim xx         As Long
  Dim szpassword As String

  '-- Always implement a runtime error handler in Callback Routines!
  On Error Resume Next

  UZDLLPass = -1  'IZ_PW_CANCEL

  If uVbSkip Then Exit Function

  '-- Get the Password prompt
  For xx = 0 To UBound(promptmsg.ch)
    If promptmsg.ch(xx) = 0 Then Exit For
    prompt = prompt & Chr$(promptmsg.ch(xx))
  Next
  If Len(prompt) = 0 Then
    prompt = "Please Enter The Password!"
  Else
    prompt = prompt & " "
    For xx = 0 To UBound(entryname.ch)
      If entryname.ch(xx) = 0 Then Exit For
      prompt = prompt & Chr$(entryname.ch(xx))
    Next
  End If

  '-- Get The Zip File Password
  Do
    szpassword = InputBox(prompt)
    If Len(szpassword) < bufsiz Then Exit Do
    ' -- Entered password exceeds UnZip's password buffer size
    If MsgBox("The supplied password exceeds the maximum password length " _
            & CStr(bufsiz - 1) & " supported by the UnZip DLL." _
            , vbExclamation + vbRetryCancel, "UnZip password too long") _
         = vbCancel Then
      szpassword = ""
      Exit Do
    End If
  Loop

  '-- No Password So Exit The Function
  If Len(szpassword) = 0 Then
    uVbSkip = True
    Exit Function
  End If

  '-- Zip File Password So Process It
  For xx = 0 To bufsiz - 1
    pwbuf.ch(xx) = 0
  Next
  '-- Password length has already been checked, so
  '-- it will fit into the communication buffer.
  For xx = 0 To Len(szpassword) - 1
    pwbuf.ch(xx) = Asc(Mid$(szpassword, xx + 1, 1))
  Next

  pwbuf.ch(xx) = 0 ' Put Null Terminator For C

  UZDLLPass = 0   ' IZ_PW_ENTERED

End Function

'-- Callback For UNZIP32.DLL - Report Function To Overwrite Files.
'-- This Function Will Display A MsgBox Asking The User
'-- If They Would Like To Overwrite The Files.
Public Function UZDLLReplacePrmt(ByRef fname As UNZIPCBChar, _
                                 ByVal fnbufsiz As Long) As Long

  Dim s0 As String
  Dim xx As Long
  Dim cCh As Byte
  Dim bufmax As Long

  '-- Always implement a runtime error handler in Callback Routines!
  On Error Resume Next

  UZDLLReplacePrmt = 100   ' 100 = Do Not Overwrite - Keep Asking User
  s0 = ""
  bufmax = UBound(fname.ch)
  If bufmax >= fnbufsiz Then bufmax = fnbufsiz - 1

  For xx = 0 To bufmax
    cCh = fname.ch(xx)
    Select Case cCh
    Case 0
      Exit For
    Case 92 ' = Asc("\")
      s0 = s0 & "/"
    Case Else
      s0 = s0 & Chr$(cCh)
    End Select
  Next

  '-- This Is The MsgBox Code
  xx = MsgBox("Overwrite """ & s0 & """ ?", vbExclamation Or vbYesNoCancel, _
              "VBUnZip32 - File Already Exists!")
  Select Case xx
  Case vbYes
    UZDLLReplacePrmt = 102    ' 102 = Overwrite, 103 = Overwrite All
  Case vbCancel
    UZDLLReplacePrmt = 104    ' 104 = Overwrite None
  Case Else
    'keep the default as set at function entry.
  End Select

End Function

'-- ASCIIZ To String Function
Public Function szTrim(szString As String) As String

  Dim pos As Long

  pos = InStr(szString, vbNullChar)

  Select Case pos
    Case Is > 1
      szTrim = Trim$(Left$(szString, pos - 1))
    Case 1
      szTrim = ""
    Case Else
      szTrim = Trim$(szString)
  End Select

End Function

'-- convert a 64-bit int divided in two Int32 variables into
'-- a single 64-bit floating-point value
Private Function CnvI64Struct2Dbl(ByVal lInt64Lo As Long, lInt64Hi As Long) As Double
  If lInt64Lo < 0 Then
    CnvI64Struct2Dbl = 2# ^ 32 + CDbl(lInt64Lo)
  Else
    CnvI64Struct2Dbl = CDbl(lInt64Lo)
  End If
  CnvI64Struct2Dbl = CnvI64Struct2Dbl + (2# ^ 32) * CDbl(lInt64Hi)
End Function

'-- Concatenate a "structured" version number into a single integer value,
'-- to facilitate version number comparisons
'-- (In case the practically used NumMajor numbers will ever exceed 128, it
'-- should be considered to use the number type "Double" to store the
'-- concatenated number. "Double" can store signed integer numbers up to a
'-- width of 52 bits without loss of precision.)
Private Function ConcatVersNums(ByVal NumMajor As Byte, ByVal NumMinor As Byte _
                              , ByVal NumRevis As Byte, ByVal NumBuild As Byte) As Long
  If (NumMajor And &H80) <> 0 Then
    ConcatVersNums = (NumMajor And &H7F) * (2 ^ 24) Or &H80000000
  Else
    ConcatVersNums = NumMajor * (2 ^ 24)
  End If
  ConcatVersNums = ConcatVersNums _
                 + NumMinor * (2 ^ 16) _
                 + NumRevis * (2 ^ 8) _
                 + NumBuild
End Function

'-- Helper function to provide a printable version number string, using the
'-- current formatting rule for version number display as implemented in UnZip.
Private Function VersNumsToTxt(ByVal NumMajor As Byte, ByVal NumMinor As Byte _
                             , ByVal NumRevis As Byte) As String
  VersNumsToTxt = CStr(NumMajor) & "." & Hex$(NumMinor)
  If NumRevis <> 0 Then VersNumsToTxt = VersNumsToTxt & Hex$(NumRevis)
End Function

'-- Helper function to convert a "concatenated" version id into a printable
'-- version number string, using the current formatting rule for version number
'-- display as implemented in UnZip.
Private Function VersIDToTxt(ByVal VersionID As Long) As String
  Dim lNumTemp As Long

  lNumTemp = VersionID \ (2 ^ 24)
  If lNumTemp < 0 Then lNumTemp = 256 + lNumTemp
  VersIDToTxt = CStr(lNumTemp) & "." _
             & Hex$((VersionID And &HFF0000) \ &H10000)
  lNumTemp = (VersionID And &HFF00&) \ &H100
  If lNumTemp <> 0 Then VersIDToTxt = VersIDToTxt & Hex$(lNumTemp)
End Function

'-- Main UNZIP32.DLL UnZip32 Subroutine
'-- (WARNING!) Do Not Change!
Public Sub VBUnZip32()

  Dim retcode As Long
  Dim MsgStr As String
  Dim TotalSizeComp As Double
  Dim TotalSize As Double
  Dim NumMembers As Double

  '-- Set The UNZIP32.DLL Options
  '-- (WARNING!) Do Not Change
  UZDCL.StructVersID = cUz_DCLStructVer      ' Current version of this structure
  UZDCL.ExtractOnlyNewer = uExtractOnlyNewer ' 1 = Extract Only Newer/New
  UZDCL.SpaceToUnderscore = uSpaceUnderScore ' 1 = Convert Space To Underscore
  UZDCL.PromptToOverwrite = uPromptOverWrite ' 1 = Prompt To Overwrite Required
  UZDCL.fQuiet = uQuiet                      ' 2 = No Messages 1 = Less 0 = All
  UZDCL.ncflag = uWriteStdOut                ' 1 = Write To Stdout
  UZDCL.ntflag = uTestZip                    ' 1 = Test Zip File
  UZDCL.nvflag = uExtractList                ' 0 = Extract 1 = List Contents
  UZDCL.nfflag = uFreshenExisting            ' 1 = Update Existing by Newer
  UZDCL.nzflag = uDisplayComment             ' 1 = Display Zip File Comment
  UZDCL.ndflag = uHonorDirectories           ' 1 = Honour Directories
  UZDCL.noflag = uOverWriteFiles             ' 1 = Overwrite Files
  UZDCL.naflag = uConvertCR_CRLF             ' 1 = Convert CR To CRLF
  UZDCL.nZIflag = uVerbose                   ' 1 = Zip Info Verbose
  UZDCL.C_flag = uCaseSensitivity            ' 1 = Case insensitivity, 0 = Case Sensitivity
  UZDCL.fPrivilege = uPrivilege              ' 1 = ACL 2 = Priv
  UZDCL.Zip = uZipFileName                   ' ZIP Filename
  UZDCL.ExtractDir = uExtractDir             ' Extraction Directory, NULL If Extracting
                                             ' To Current Directory

  '-- Set Callback Addresses
  '-- (WARNING!!!) Do Not Change
  UZUSER.UZDLLPrnt = FnPtr(AddressOf UZDLLPrnt)
  UZUSER.UZDLLSND = 0&    '-- Not Supported
  UZUSER.UZDLLREPLACE = FnPtr(AddressOf UZDLLReplacePrmt)
  UZUSER.UZDLLPASSWORD = FnPtr(AddressOf UZDLLPass)
  UZUSER.UZDLLMESSAGE_I32 = FnPtr(AddressOf UZReceiveDLLMessage_I32)
  UZUSER.UZDLLSERVICE_I32 = FnPtr(AddressOf UZDLLServ_I32)

  '-- Set UNZIP32.DLL Version Space
  '-- (WARNING!!!) Do Not Change
  With UZVER2
    .structlen = Len(UZVER2)
    .beta = String$(10, vbNullChar)
    .date = String$(20, vbNullChar)
    .zlib = String$(10, vbNullChar)
  End With

  '-- Get Version
  retcode = UzpVersion2(UZVER2)
  If retcode <> 0 Then
    MsgBox "Incompatible DLL version discovered!" & vbNewLine _
         & "The UnZip DLL requires a version structure of length " _
         & CStr(retcode) & ", but the VB frontend expects the DLL to need " _
         & Len(UZVER2) & "bytes." & vbNewLine _
         & vbNewLine & "The program cannot continue." _
         , vbCritical + vbOKOnly, App.Title
    Exit Sub
  End If

  ' Check that the DLL version is sufficiently recent
  If (ConcatVersNums(UZVER2.unzip(1), UZVER2.unzip(2) _
                  , UZVER2.unzip(3), UZVER2.unzip(4)) < _
      ConcatVersNums(cUzDLL_MinVer_Major, cUzDLL_MinVer_Minor _
                  , cUzDLL_MinVer_Revis, 0)) Then
    ' The found UnZip DLL is too old!
    MsgBox "Incompatible old DLL version discovered!" & vbNewLine _
         & "This program requires an UnZip DLL version of at least " _
         & VersNumsToTxt(cUzDLL_MinVer_Major, cUzDLL_MinVer_Minor, cUzDLL_MinVer_Revis) _
         & ", but the version reported by the found DLL is only " _
         & VersNumsToTxt(UZVER2.unzip(1), UZVER2.unzip(2), UZVER2.unzip(3)) _
         & "." & vbNewLine _
         & vbNewLine & "The program cannot continue." _
         , vbCritical + vbOKOnly, App.Title
    Exit Sub
  End If

  ' Concatenate the DLL API version info into a single version id variable.
  ' This variable may be used later on to switch between different
  ' known variants of specific API calls or API structures.
  m_UzDllApiVers = ConcatVersNums(UZVER2.dllapimin(1), UZVER2.dllapimin(2) _
                                , UZVER2.dllapimin(3), UZVER2.dllapimin(4))
  ' check that the DLL API version is not too new
  If (m_UzDllApiVers > _
      ConcatVersNums(cUzDLL_MaxAPI_Major, cUzDLL_MaxAPI_Minor _
                  , cUzDLL_MaxAPI_Revis, 0)) Then
    ' The found UnZip DLL is too new!
    MsgBox "DLL version with incompatible API discovered!" & vbNewLine _
         & "This program can only handle UnZip DLL API versions up to " _
         & VersNumsToTxt(cUzDLL_MaxAPI_Major, cUzDLL_MaxAPI_Minor, cUzDLL_MaxAPI_Revis) _
         & ", but the found DLL reports a newer API version of " _
         & VersIDToTxt(m_UzDllApiVers) & "." & vbNewLine _
         & vbNewLine & "The program cannot continue." _
         , vbCritical + vbOKOnly, App.Title
    Exit Sub
  End If

  '--------------------------------------
  '-- You Can Change This For Displaying
  '-- The Version Information!
  '--------------------------------------
  MsgStr$ = "DLL Date: " & szTrim(UZVER2.date)
  MsgStr$ = MsgStr$ & vbNewLine$ & "Zip Info: " _
       & VersNumsToTxt(UZVER2.zipinfo(1), UZVER2.zipinfo(2), UZVER2.zipinfo(3))
  MsgStr$ = MsgStr$ & vbNewLine$ & "DLL Version: " _
       & VersNumsToTxt(UZVER2.windll(1), UZVER2.windll(2), UZVER2.windll(3))
  MsgStr$ = MsgStr$ & vbNewLine$ & "DLL API Compatibility: " _
       & VersIDToTxt(m_UzDllApiVers)
  MsgStr$ = MsgStr$ & vbNewLine$ & "--------------"
  '-- End Of Version Information.

  '-- Go UnZip The Files! (Do Not Change Below!!!)
  '-- This Is The Actual UnZip Routine
  retcode = Wiz_SingleEntryUnzip(uNumberFiles, uZipNames, uNumberXFiles, _
                                 uExcludeNames, UZDCL, UZUSER)
  '---------------------------------------------------------------

  '-- If There Is An Error Display A MsgBox!
  If retcode <> 0 Then _
    MsgBox "UnZip DLL call returned error code #" & CStr(retcode) _
          , vbExclamation, App.Title

  '-- Add up 64-bit values
  TotalSizeComp = CnvI64Struct2Dbl(UZUSER.TotalSizeComp_Lo, _
                                   UZUSER.TotalSizeComp_Hi)
  TotalSize = CnvI64Struct2Dbl(UZUSER.TotalSize_Lo, _
                               UZUSER.TotalSize_Hi)
  NumMembers = CnvI64Struct2Dbl(UZUSER.NumMembers_Lo, _
                                UZUSER.NumMembers_Hi)

  '-- You Can Change This As Needed!
  '-- For Compression Information
  MsgStr$ = MsgStr$ & vbNewLine & _
       "Only Shows If uExtractList = 1 List Contents"
  MsgStr$ = MsgStr$ & vbNewLine & "--------------"
  MsgStr$ = MsgStr$ & vbNewLine & "Comment         : " & UZUSER.cchComment
  MsgStr$ = MsgStr$ & vbNewLine & "Total Size Comp : " _
                    & Format$(TotalSizeComp, "#,0")
  MsgStr$ = MsgStr$ & vbNewLine & "Total Size      : " _
                    & Format$(TotalSize, "#,0")
  MsgStr$ = MsgStr$ & vbNewLine & "Compress Factor : %" & UZUSER.CompFactor
  MsgStr$ = MsgStr$ & vbNewLine & "Num Of Members  : " & NumMembers
  MsgStr$ = MsgStr$ & vbNewLine & "--------------"

  VBUnzFrm.txtMsgOut.Text = VBUnzFrm.txtMsgOut.Text & MsgStr$ & vbNewLine
End Sub
