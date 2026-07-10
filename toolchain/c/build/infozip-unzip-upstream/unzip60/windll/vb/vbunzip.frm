VERSION 5.00
Object = "{F9043C88-F6F2-101A-A3C9-08002B2F49FB}#1.1#0"; "COMDLG32.OCX"
Begin VB.Form VBUnzFrm 
   AutoRedraw      =   -1  'True
   Caption         =   "VBUnzFrm"
   ClientHeight    =   4785
   ClientLeft      =   780
   ClientTop       =   525
   ClientWidth     =   9375
   BeginProperty Font 
      Name            =   "Fixedsys"
      Size            =   9
      Charset         =   0
      Weight          =   400
      Underline       =   0   'False
      Italic          =   0   'False
      Strikethrough   =   0   'False
   EndProperty
   LinkTopic       =   "VBUnzFrm"
   ScaleHeight     =   4785
   ScaleWidth      =   9375
   StartUpPosition =   1  'Fenstermitte
   Begin VB.CheckBox checkOverwriteAll 
      Alignment       =   1  'Rechts ausgerichtet
      Caption         =   "Overwrite all?"
      BeginProperty Font 
         Name            =   "MS Sans Serif"
         Size            =   9.75
         Charset         =   0
         Weight          =   400
         Underline       =   0   'False
         Italic          =   0   'False
         Strikethrough   =   0   'False
      EndProperty
      Height          =   255
      Left            =   240
      TabIndex        =   5
      Top             =   1320
      Width           =   4425
   End
   Begin VB.TextBox txtZipFName 
      BeginProperty Font 
         Name            =   "Courier New"
         Size            =   9.75
         Charset         =   0
         Weight          =   400
         Underline       =   0   'False
         Italic          =   0   'False
         Strikethrough   =   0   'False
      EndProperty
      Height          =   375
      Left            =   4440
      TabIndex        =   1
      Top             =   120
      Width           =   4335
   End
   Begin VB.TextBox txtExtractRoot 
      BeginProperty Font 
         Name            =   "Courier New"
         Size            =   9.75
         Charset         =   0
         Weight          =   400
         Underline       =   0   'False
         Italic          =   0   'False
         Strikethrough   =   0   'False
      EndProperty
      Height          =   375
      Left            =   4440
      TabIndex        =   4
      Top             =   720
      Width           =   4335
   End
   Begin VB.CommandButton cmdStartUnz 
      Caption         =   "Start"
      Height          =   495
      Left            =   240
      TabIndex        =   6
      Top             =   1800
      Width           =   3255
   End
   Begin VB.TextBox txtMsgOut 
      BeginProperty Font 
         Name            =   "Courier New"
         Size            =   9
         Charset         =   0
         Weight          =   400
         Underline       =   0   'False
         Italic          =   0   'False
         Strikethrough   =   0   'False
      EndProperty
      Height          =   2175
      Left            =   240
      Locked          =   -1  'True
      MultiLine       =   -1  'True
      ScrollBars      =   3  'Beides
      TabIndex        =   8
      TabStop         =   0   'False
      Top             =   2520
      Width           =   8895
   End
   Begin VB.CommandButton cmdQuitVBUnz 
      Cancel          =   -1  'True
      Caption         =   "Quit"
      Height          =   495
      Left            =   6240
      TabIndex        =   7
      Top             =   1800
      Width           =   2895
   End
   Begin VB.CommandButton cmdSearchZfile 
      Caption         =   "..."
      BeginProperty Font 
         Name            =   "MS Sans Serif"
         Size            =   8.25
         Charset         =   0
         Weight          =   400
         Underline       =   0   'False
         Italic          =   0   'False
         Strikethrough   =   0   'False
      EndProperty
      Height          =   375
      Left            =   8760
      TabIndex        =   2
      Top             =   120
      Width           =   375
   End
   Begin MSComDlg.CommonDialog CommonDialog1 
      Left            =   4800
      Top             =   1800
      _ExtentX        =   847
      _ExtentY        =   847
      _Version        =   393216
   End
   Begin VB.Label Label1 
      Caption         =   "Complete path-name of Zip-archive:"
      BeginProperty Font 
         Name            =   "MS Sans Serif"
         Size            =   9.75
         Charset         =   0
         Weight          =   400
         Underline       =   0   'False
         Italic          =   0   'False
         Strikethrough   =   0   'False
      EndProperty
      Height          =   255
      Left            =   240
      TabIndex        =   0
      Top             =   120
      Width           =   3855
   End
   Begin VB.Label Label2 
      Caption         =   "Extract archive into directory:"
      BeginProperty Font 
         Name            =   "MS Sans Serif"
         Size            =   9.75
         Charset         =   0
         Weight          =   400
         Underline       =   0   'False
         Italic          =   0   'False
         Strikethrough   =   0   'False
      EndProperty
      Height          =   255
      Left            =   240
      TabIndex        =   3
      Top             =   720
      Width           =   3855
   End
End
Attribute VB_Name = "VBUnzFrm"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = False
Option Explicit

'---------------------------------------------------
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
'-- For Letting Me Use And Modify Their Orginal
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
'-- by Christian Spieler
'-- (added sort of a "windows oriented" user interface)
'-- Modified May 11, 2003
'-- by Christian Spieler
'-- (use late binding for referencing the common dialog)
'-- Modified December 30, 2008
'-- by Ed Gordon
'-- (add Overwrite_All checkbox and resizing of txtMsgOut
'-- output box)
'-- Modified January 03, 2009
'-- by Christian Spieler
'-- (fixed tab navigation sequence, changed passing of
'-- "overwrite-all" setting to use existing option flags,
'-- cleared all msg buffer at start of every DLL call,
'-- removed code that is not supported by VB5)
'--
'---------------------------------------------------------------

Private mCommDlgCtrl As Object

Private Sub cmdStartUnz_Click()

    Dim MsgTmp As String
    
    Cls
    txtMsgOut.Text = ""
    
    '-- Init Global Message Variables
    uZipInfo = ""
    uZipMessage = ""
    uZipNumber = 0   ' Holds The Number Of Zip Files
    
    '-- Select UNZIP32.DLL Options - Change As Required!
    ' 1 = Always Overwrite Files
    uOverWriteFiles = Me.checkOverwriteAll.Value
    ' 1 = Prompt To Overwrite
    uPromptOverWrite = IIf(uOverWriteFiles = 0, 1, 0)
    uDisplayComment = 0   ' 1 = Display comment ONLY!!!
    uHonorDirectories = 1  ' 1 = Honour Zip Directories
    
    '-- Select Filenames If Required
    '-- Or Just Select All Files
    uZipNames.uzFiles(0) = vbNullString
    uNumberFiles = 0
    
    '-- Select Filenames To Exclude From Processing
    ' Note UNIX convention!
    '   vbxnames.s(0) = "VBSYX/VBSYX.MID"
    '   vbxnames.s(1) = "VBSYX/VBSYX.SYX"
    '   numx = 2
    
    '-- Or Just Select All Files
    uExcludeNames.uzFiles(0) = vbNullString
    uNumberXFiles = 0
    
    '-- Change The Next 2 Lines As Required!
    '-- These Should Point To Your Directory
    uZipFileName = txtZipFName.Text
    uExtractDir = txtExtractRoot.Text
    If Len(uExtractDir) <> 0 Then
      uExtractList = 0  ' 0 = Extract if dir specified
    Else
      uExtractList = 1  ' 1 = List Contents Of Zip
    End If
    
    '-- Let's Go And Unzip Them!
    Call VBUnZip32
    
    '-- Tell The User What Happened
    If Len(uZipMessage) > 0 Then
        MsgTmp = uZipMessage
        uZipMessage = ""
    End If
    
    '-- Display Zip File Information.
    If Len(uZipInfo) > 0 Then
        MsgTmp = MsgTmp & vbNewLine & "uZipInfo is:" & vbNewLine & uZipInfo
        uZipInfo = ""
    End If
    
    '-- Display The Number Of Extracted Files!
    If uZipNumber > 0 Then
        MsgTmp = MsgTmp & vbNewLine & "Number Of Files: " & Str(uZipNumber)
    End If
    
    txtMsgOut.Text = txtMsgOut.Text & MsgTmp & vbNewLine
    
    
End Sub


Private Sub Form_Load()
    
    '-- To work around compatibility issues between different versions of
    '-- Visual Basic, we use a late bound untyped object variable to reference
    '-- the common dialog ActiveX-control object at runtime.
    On Error Resume Next
    Set mCommDlgCtrl = CommonDialog1
    On Error GoTo 0
    '-- Disable the "call openfile dialog" button, when the common dialog
    '-- object is not available
    cmdSearchZfile.Visible = Not (mCommDlgCtrl Is Nothing)
    
    txtZipFName.Text = vbNullString
    txtExtractRoot.Text = vbNullString
    Me.Show
    
End Sub

Private Sub Form_Resize()
    Dim Wid As Single
    Dim Hei As Single
    
    Wid = Me.Width - 600 ' 9495 - 8895
    If Wid < 2000 Then Wid = 2000
    txtMsgOut.Width = Wid
    
    Hei = Me.Height - 3120 ' 5295 - 2175
    If Hei < 1000 Then Hei = 1000
    txtMsgOut.Height = Hei

End Sub

Private Sub Form_Unload(Cancel As Integer)
    '-- remove runtime reference to common dialog control object
    Set mCommDlgCtrl = Nothing
End Sub


Private Sub cmdQuitVBUnz_Click()
    Unload Me
End Sub


Private Sub cmdSearchZfile_Click()
    If mCommDlgCtrl Is Nothing Then Exit Sub
    mCommDlgCtrl.CancelError = False
    mCommDlgCtrl.DialogTitle = "Open Zip-archive"
    '-- The following property is not supported in the first version(s)
    '-- of the common dialog controls. But this feature is of minor
    '-- relevance in our context, so we simply skip over the statement
    '-- in case of errors.
    On Error Resume Next
    mCommDlgCtrl.DefaultExt = ".zip"
    On Error GoTo err_deactivateControl
    '-- Initialize the file name with the current setting of the filename
    '-- text box.
    mCommDlgCtrl.FileName = txtZipFName.Text
    '-- Provide reasonable filter settings for selecting Zip archives.
    mCommDlgCtrl.Filter = "Zip archives (*.zip)|*.zip|All files (*.*)|*.*"
    mCommDlgCtrl.ShowOpen
    '-- In case the user closed the dialog via cancel, the FilenName
    '-- property contains its initial setting and no change occurs.
    txtZipFName.Text = mCommDlgCtrl.FileName
    Exit Sub

err_deactivateControl:
    '-- Emit a warning message.
    MsgBox "Unexpected error #" & CStr(Err.Number) & " in call to ComDLG32" _
         & " FileOpen dialog:" & vbNewLine & Err.Description & vbNewLine _
         & vbNewLine & "The version of the COMDLG32.OCX control installed" _
         & " on your system seems to be too old. Please consider upgrading" _
         & " to a recent release of the Common Dialog ActiveX control." _
         & vbNewLine & "The ""Choose File from List"" dialog functionality" _
         & " has been disabled for this session.", _
         vbCritical + vbOKOnly, "FileOpen Dialog incompatible"
    '-- Deactivate the control and prevent further usage in this session.
    Set mCommDlgCtrl = Nothing
    cmdSearchZfile.Enabled = False
End Sub

