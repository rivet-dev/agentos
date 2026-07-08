#region README

  //_____________________________________________________________________________
  //
  //Sample C# code, .NET Framework 1.1, contributed to the Info-Zip project by
  //Adrian Maull, April 2005.
  //
  //If you have questions or comments, contact me at adrian.maull@sprintpcs.com.  Though
  //I will try to respond to coments/questions, I do not guarantee such response.
  //
  //THIS CODE AND INFORMATION ARE PROVIDED "AS IS" WITHOUT WARRANTY OF ANY
  //KIND, EITHER EXPRESSED OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
  //IMPLIED WARRANTIES OF MERCHANTABILITY AND/OR FITNESS FOR A
  //PARTICULAR PURPOSE.
  //
  //_____________________________________________________________________________


#endregion

#region KNOWN ISSUES

  //_____________________________________________________________________________
  //
  //KNOWN ISSUES:
  //From my testing I have encountered some issues
  //
  //1.  I receive an error code from the Unzip32.dll when I try to retrieve the comment from the
  //    zip file.  To display a comment you set the nzflag of the DCLIST structure = 1.  In this
  //    implementation, just set the m_Unzip.ExtractList = ExtractListEnum.ListContents and
  //    m_Unzip.DisplayComment = DisplayCommentEnum.True
  //    I provided a work around to this in the GetZipFileComment() method.
  //
  //    [CS, 2009-01-10:]
  //    The DisplayComment option works now. However, the work-around code to
  //    retrieve the zipfile comment using direct I/O has not yet been replaced
  //    by code using the UnZip DLL.
  //
  //2.  I have not tested any password/encryption logic in this sample
  //_____________________________________________________________________________


#endregion

using System;
using System.Security.Permissions;
using System.Runtime.InteropServices;
using System.Text;
using System.Windows.Forms;
using System.IO;

namespace CSharpInfoZip_UnZipSample
{
  /// <summary>
  /// Summary description for Unzip.
  /// </summary>

  #region Public Enums

  public enum ExtractOnlyNewerEnum {False, True};
  public enum SpaceToUnderScoreEnum {False, True};
  public enum PromptToOverWriteEnum {NotRequired, Required};
  public enum QuietEnum {AllMessages, LessMessages, NoMessages};
  public enum WriteStdOutEnum {False, True};
  public enum TestZipEnum {False, True};
  public enum ExtractOrListEnum {Extract, ListContents};
  public enum FreshenExistingEnum {False, True};
  public enum DisplayCommentEnum {False, True};
  public enum HonorDirectoriesEnum {False, True};
  public enum OverWriteFilesEnum {False, True};
  public enum ConvertCR_CRLFEnum {False, True};
  public enum VerboseZIEnum {False, True};
  public enum UnixFileBackupEnum { False, True }
  public enum CaseSensitivityEnum { True, False };
  public enum RestoreTimeStampsEnum { All, FilesOnly, None };
  public enum UTF8NameTranslationEnum { Automatic, EscapeNonASCII, Disabled };
  public enum PrivilegeEnum {Default, ACL, Privilege};
  public enum ReplaceFileOptionsEnum {ReplaceNo=100, ReplaceYes=102, ReplaceAll=103,
                                      ReplaceNone=104, ReplaceRename=105};


  #endregion

  #region Event Delegates

  public delegate void UnZipDLLPrintMessageEventHandler(object sender, UnZipDLLPrintMessageEventArgs  e);
  public delegate int UnZipDLLServiceMessageEventHandler(object sender, UnZipDLLServiceMessageEventArgs  e);

  #endregion

  public class Unzip
  {
    public Unzip()
    {
    }

    #region Constants
    // Current version of the DCLIST interface structure, must be matched
    // with the UZ_DCL_STRUCTVER constant as defined in the struct.h header
    // from the UnZip WinDLL code.
    private const uint uz_dcl_StructVer = 0x600;
    #endregion

    #region Private Vars

    private ulong m_ZipFileSize;
    private ulong m_ZipFileCount;
    private int m_Stop;
    private string m_ZipFileName;
    private string m_ExtractionDirectory;
    private string m_Password;
    private string m_Comment;
    private string [] m_FilesToUnzip = new string[0]; //Default
    private string [] m_FilesToExclude = new string[0]; //Default
    private ZipFileEntries m_ZipFileEntries = new ZipFileEntries();
    private Encoding m_EncodingANSI = Encoding.Default;

    private ExtractOnlyNewerEnum m_ExtractOnlyNewer;
    private SpaceToUnderScoreEnum m_SpaceToUnderScore;
    private PromptToOverWriteEnum m_PromptToOverWrite;
    private QuietEnum m_Quiet;
    private WriteStdOutEnum m_WriteStdOut;
    private TestZipEnum m_TestZip;
    private ExtractOrListEnum m_ExtractOrList;
    private FreshenExistingEnum m_FreshenExisting;
    private DisplayCommentEnum m_DisplayComment;
    private HonorDirectoriesEnum m_HonorDirectories;
    private OverWriteFilesEnum m_OverWriteFiles;
    private ConvertCR_CRLFEnum m_ConvertCR_CRLF;
    private VerboseZIEnum m_VerboseZI;
    private UnixFileBackupEnum m_UnixFileBackup;
    private CaseSensitivityEnum m_CaseSensitivity;
    private RestoreTimeStampsEnum m_RestoreTimeStamps;
    private UTF8NameTranslationEnum m_UTF8NameTranslation;
    private PrivilegeEnum m_Privilege;


    #endregion

    #region Structures

    // Callback Large String
    protected struct UNZIPCBChar
    {
      [MarshalAs( UnmanagedType.ByValArray, SizeConst= 32000, ArraySubType = UnmanagedType.U1)]
      public byte [] ch;
    }

    // Callback Small String (size of a filename buffer)
    protected struct UNZIPCBCh
    {
      [MarshalAs( UnmanagedType.ByValArray, SizeConst= 260, ArraySubType = UnmanagedType.U1)]
      public byte [] ch;
    }

    //UNZIP32.DLL DCLIST Structure
    [ StructLayout( LayoutKind.Sequential )]
    protected struct DCLIST
    {
      public uint StructVersID;       // struct version id (= UZ_DCL_STRUCTVER)
      public int ExtractOnlyNewer;    //1 = Extract Only Newer/New, Else 0
      public int SpaceToUnderscore;   //1 = Convert Space To Underscore, Else 0
      public int PromptToOverwrite;   //1 = Prompt To Overwrite Required, Else 0
      public int fQuiet;              //2 = No Messages, 1 = Less, 0 = All
      public int ncflag;              //1 = Write To Stdout, Else 0
      public int ntflag;              //1 = Test Zip File, Else 0
      public int nvflag;              //0 = Extract, 1 = List Zip Contents
      public int nfflag;              //1 = Extract Only Newer Over Existing, Else 0
      public int nzflag;              //1 = Display Zip File Comment, Else 0
      public int ndflag;              //1 = Honor Directories, Else 0
      public int noflag;              //1 = Overwrite Files, Else 0
      public int naflag;              //1 = Convert CR To CRLF, Else 0
      public int nZIflag;             //1 = Zip Info Verbose, Else 0
      public int B_flag;              //1 = backup existing files
      public int C_flag;              //1 = Case Insensitivity, 0 = Case Sensitivity
      public int D_flag;              //controls restoration of timestamps
                                      //0 = restore all timestamps (default)
                                      //1 = skip restoration of timestamps for folders
                                      //    created on behalf of directory entries in the
                                      //    Zip archive
                                      //2 = no restoration of timestamps; extracted files
                                      //    and dirs get stamped with current time */
      public int U_flag;              // controls UTF-8 filename coding support
                                      //0 = automatic UTF-8 translation enabled (default)
                                      //1 = recognize UTF-8 coded names, but all non-ASCII
                                      //    characters are "escaped" into "#Uxxxx"
                                      //2 = UTF-8 support is disabled, filename handling
                                      //    works exactly as in previous UnZip versions */
      public int fPrivilege;          //1 = ACL, 2 = Privileges
      public string Zip;              //The Zip Filename To Extract Files
      public string ExtractDir;       //The Extraction Directory, NULL If Extracting To Current Dir
    }

    //UNZIP32.DLL Userfunctions Structure
    [ StructLayout( LayoutKind.Sequential )]
    protected struct USERFUNCTION
    {
      public UZDLLPrintCallback UZDLLPrnt;              //Print function callback
      public int UZDLLSND;                              //Not supported
      public UZDLLReplaceCallback UZDLLREPLACE;         //Replace function callback
      public UZDLLPasswordCallback UZDLLPASSWORD;       //Password function callback
      // 64-bit versions
      public UZReceiveDLLMessageCallback UZDLLMESSAGE;  //Receive message callback
      public UZDLLServiceCallback UZDLLSERVICE;         //Service callback
      // 32-bit versions (not used/needed here)
      public UZReceiveDLLMsgCBck_32 UZDLLMESSAGE_I32;   //Receive message callback
      public UZDLLServiceCBck_32 UZDLLSERVICE_I32;      //Service callback
      public ulong TotalSizeComp;                       //Total Size Of Zip Archive
      public ulong TotalSize;                           //Total Size Of All Files In Archive
      public ulong NumMembers;                          //Total Number Of All Files In The Archive
      public uint CompFactor;                           //Compression Factor
      public short cchComment;                          //Flag If Archive Has A Comment!
    }

    #endregion

    #region DLL Function Declares

    //NOTE:
    //This Assumes UNZIP32.DLL Is In Your \Windows\System Directory!
    [DllImport("unzip32.dll", SetLastError=true)]
    private static extern int Wiz_SingleEntryUnzip (int ifnc, string [] ifnv, int xfnc, string [] xfnv,
      ref DCLIST dcl, ref USERFUNCTION Userf);


    #endregion

    #region Properties

    public string Password
    {
      get {return m_Password;}
      set {m_Password = value;}
    }

    public string Comment
    {
      get {return m_Comment;}
      set {m_Comment = value;}
    }

    public ExtractOnlyNewerEnum ExtractOnlyNewer
    {
      get {return m_ExtractOnlyNewer;}
      set {m_ExtractOnlyNewer = value;}
    }

    public SpaceToUnderScoreEnum SpaceToUnderScore
    {
      get {return m_SpaceToUnderScore;}
      set {m_SpaceToUnderScore = value;}
    }

    public PromptToOverWriteEnum PromptToOverWrite
    {
      get {return m_PromptToOverWrite;}
      set {m_PromptToOverWrite = value;}
    }

    public QuietEnum Quiet
    {
      get {return m_Quiet;}
      set {m_Quiet = value;}
    }

    public WriteStdOutEnum WriteStdOut
    {
      get {return m_WriteStdOut;}
      set {m_WriteStdOut = value;}
    }

    public TestZipEnum TestZip
    {
      get {return m_TestZip;}
      set {m_TestZip = value;}
    }

    public ExtractOrListEnum ExtractOrList
    {
      get {return m_ExtractOrList;}
      set {m_ExtractOrList = value;}
    }

    public FreshenExistingEnum FreshenExisting
    {
      get {return m_FreshenExisting;}
      set {m_FreshenExisting = value;}
    }

    public DisplayCommentEnum DisplayComment
    {
      get {return m_DisplayComment;}
      set {m_DisplayComment = value;}
    }

    public HonorDirectoriesEnum HonorDirectories
    {
      get {return m_HonorDirectories;}
      set {m_HonorDirectories = value;}
    }

    public OverWriteFilesEnum OverWriteFiles
    {
      get {return m_OverWriteFiles;}
      set {m_OverWriteFiles = value;}
    }

    public ConvertCR_CRLFEnum ConvertCR_CRLF
    {
      get {return m_ConvertCR_CRLF;}
      set {m_ConvertCR_CRLF = value;}
    }

    public VerboseZIEnum VerboseZI
    {
      get {return m_VerboseZI;}
      set {m_VerboseZI = value;}
    }

    public UnixFileBackupEnum UnixFileBackup
    {
      get { return m_UnixFileBackup; }
      set { m_UnixFileBackup = value; }
    }

    public CaseSensitivityEnum CaseSensitivity
    {
      get {return m_CaseSensitivity;}
      set {m_CaseSensitivity = value;}
    }

    public RestoreTimeStampsEnum RestoreTimeStamps
    {
      get { return m_RestoreTimeStamps; }
      set { m_RestoreTimeStamps = value; }
    }

    public UTF8NameTranslationEnum UTF8NameTranslation
    {
      get { return m_UTF8NameTranslation; }
      set { m_UTF8NameTranslation = value; }
    }

    public PrivilegeEnum Privilege
    {
      get {return m_Privilege;}
      set {m_Privilege = value;}
    }

    public string ZipFileName
    {
      get {return m_ZipFileName;}
      set {m_ZipFileName = value;}
    }

    public string ExtractDirectory
    {
      get {return m_ExtractionDirectory;}
      set {m_ExtractionDirectory = value;}
    }

    public string [] FilesToUnzip
    {
      get {return m_FilesToUnzip;}
      set {m_FilesToUnzip = value;}
    }

    public string [] FilesToExclude
    {
      get {return m_FilesToExclude;}
      set {m_FilesToExclude = value;}
    }

    #endregion

    #region UnZip DLL Delegates

    //Callback For UNZIP32.DLL - Receive Message Function
    protected delegate void UZReceiveDLLMessageCallback (ulong ucsize, ulong csiz,
                                ushort cfactor, ushort mo,
                                ushort dy, ushort yr, ushort hh, ushort mm, byte c,
                                [MarshalAs(UnmanagedType.LPStr)]String fname,
                                [MarshalAs(UnmanagedType.LPStr)]String meth,
                                uint crc, sbyte fCrypt);

    //Callback For UNZIP32.DLL - Receive Message Function (no-Int64 variant, not used here)
    protected delegate void UZReceiveDLLMsgCBck_32(uint ucsize_lo, uint ucsize_hi, uint csiz_lo, uint csiz_hi,
                                ushort cfactor, ushort mo,
                                ushort dy, ushort yr, ushort hh, ushort mm, byte c,
                                [MarshalAs(UnmanagedType.LPStr)]String fname,
                                [MarshalAs(UnmanagedType.LPStr)]String meth,
                                uint crc, sbyte fCrypt);

    //Callback For UNZIP32.DLL - Print Message Function
    protected delegate int UZDLLPrintCallback ([MarshalAs(UnmanagedType.LPStr)]String fname, uint x);

    //Callback For UNZIP32.DLL - DLL Service Function
    protected delegate int UZDLLServiceCallback([MarshalAs(UnmanagedType.LPStr)]String fname,
                                                ulong ucsize);

    //Callback For UNZIP32.DLL - DLL Service Function (no-Int64 variant, not used here)
    protected delegate int UZDLLServiceCBck_32([MarshalAs(UnmanagedType.LPStr)]String fname,
                                               uint ucsize_lo, uint ucsize_hi);

    //Callback For UNZIP32.DLL - Password Function
    protected delegate int UZDLLPasswordCallback(ref UNZIPCBCh pwd, int bufsize,
                                                 [MarshalAs(UnmanagedType.LPStr)]String msg,
                                                 [MarshalAs(UnmanagedType.LPStr)]String entryname);

    //Callback For UNZIP32.DLL - Replace Function To Overwrite Files
    protected delegate int UZDLLReplaceCallback(ref UNZIPCBCh fname, uint fnbufsiz);

    #endregion

    #region Events

    public event UnZipDLLPrintMessageEventHandler ReceivePrintMessage;
    public event UnZipDLLServiceMessageEventHandler ReceiveServiceMessage;

    #endregion

    #region Protected Functions

    protected virtual void OnReceivePrintMessage (UnZipDLLPrintMessageEventArgs e)
    {
      if (ReceivePrintMessage != null)
      {
        ReceivePrintMessage(this, e);
      }
    }

    protected virtual void OnReceiveServiceMessage (UnZipDLLServiceMessageEventArgs e)
    {
      if (ReceiveServiceMessage != null)
      {
        ReceiveServiceMessage(this, e);
      }
    }

    #endregion

    #region CallBack Functions

    //This function is called when the DCLIST structure's nvflag is ExtractListEnum.ListContents.
    //It is used to list the zip file contents
    protected void UZReceiveDLLMessage (ulong ucsize, ulong csiz, ushort cfactor, ushort mo,
                              ushort dy, ushort yr, ushort hh, ushort mm, byte c,
                              [MarshalAs(UnmanagedType.LPStr)]String fname,
                              [MarshalAs(UnmanagedType.LPStr)]String meth,
                              uint crc, sbyte fCrypt)
    {
      //Add up the size of each file in the zip file
      m_ZipFileSize += ucsize;
      m_ZipFileCount ++;

      //NOTE:
      //Build out the ZipFileEntry collection
      //You can do additional formatting for the month, day, and year properties
      ZipFileEntry zfe = new ZipFileEntry();
      zfe.FileName = Path.GetFileName(fname);
      zfe.FilePath = Path.GetDirectoryName(fname);
      zfe.IsFolder = (zfe.FileName.Length == 0 ? true : false);
      zfe.FileSize = unchecked(ucsize);
      zfe.FileMonth = mo;
      zfe.FileDay = dy;
      zfe.FileYear = yr;
      zfe.FileHour = hh;
      zfe.FileMinute = mm;
      zfe.CompressedSize = unchecked(csiz);
      zfe.CompressionFactor = cfactor;
      zfe.CompressionMethShort = meth;

      m_ZipFileEntries.Add(zfe);

    }

    protected int UZDLLPrint([MarshalAs(UnmanagedType.LPStr)]String msg, uint x)
    {
      UnZipDLLPrintMessageEventArgs e =
          new UnZipDLLPrintMessageEventArgs(msg.Substring(0, Math.Min(unchecked((int)x),
                                                                      msg.Length)));
      OnReceivePrintMessage(e);

      return 0;
    }

    /*
    DLLSERVICE *ServCallBk  = Callback function designed to be used for
                allowing the application to process Windows messages,
                or canceling the operation, as well as giving the
                option of a progress indicator. If this function
                returns a non-zero value, then it will terminate
                what it is doing. It provides the application with
                the name of the name of the archive member it has
                just processed, as well as it's original size.

    fname.ch = the name of the file being zipped
    ucsize = the size of the file being zipped

     * */
    protected int UZDLLService([MarshalAs(UnmanagedType.LPStr)]String fname,
                               ulong ucsize)
    {
      //Raise this event
      UnZipDLLServiceMessageEventArgs e =
          new UnZipDLLServiceMessageEventArgs(m_ZipFileSize, fname, unchecked(ucsize));
      OnReceiveServiceMessage (e);

      return m_Stop;
    }

    protected int UZDLLPassword(ref UNZIPCBCh pwd, int bufsize,
                        [MarshalAs(UnmanagedType.LPStr)]String msg,
                        [MarshalAs(UnmanagedType.LPStr)]String entryname)
    {
      if (m_Password == null | m_Password == string.Empty) return -1;

      if (m_Password.Length >= bufsize)
      {
        MessageBox.Show("Length of supplied password exceeds available pw buffer size "
                    + bufsize.ToString() + "!", "Password Error",
                    MessageBoxButtons.OK, MessageBoxIcon.Error);
        return 5;  // IZ_PW_ERROR
      }

      //clear the byte array
      for (int i = 0; i < bufsize; i ++)
        pwd.ch[i] = 0;

      m_EncodingANSI.GetBytes(m_Password, 0, m_Password.Length, pwd.ch, 0);

      return 0;
    }

    protected int UZDLLReplace(ref UNZIPCBCh fname, uint fnbufsiz)
    {
      ReplaceFileOptionsEnum ReplaceFileOption = ReplaceFileOptionsEnum.ReplaceNo; //Default
      string s = string.Empty;
      int i = 0;

      if (fnbufsiz > fname.ch.Length) fnbufsiz = (uint)fname.ch.Length;
      for (i = 0; i < fnbufsiz; i++)
        if (fname.ch[i] == 0) break;
      s = m_EncodingANSI.GetString(fname.ch, 0, i);

      DialogResult rslt = MessageBox.Show("Overwrite [" + s + "]?  Click Cancel to skip all.",
                                  "Overwrite Confirmation", MessageBoxButtons.YesNoCancel,
                                  MessageBoxIcon.Question);
      switch (rslt)
      {
        case DialogResult.No:
          ReplaceFileOption = ReplaceFileOptionsEnum.ReplaceNo;
          break;
        case DialogResult.Yes:
          ReplaceFileOption = ReplaceFileOptionsEnum.ReplaceYes;
          break;
        case DialogResult.Cancel:
          ReplaceFileOption = ReplaceFileOptionsEnum.ReplaceNone;
          break;
      }
      return ConvertEnumToInt(ReplaceFileOption);
    }

    #endregion

    #region Public Functions

    public int UnZipFiles ()
    {
      int ret = -1;

      //check to see if there is enough information to proceed.
      //Exceptions can be thrown if required data is not passed in
      if (m_ZipFileName == string.Empty) return -1;
      if (m_ExtractionDirectory == string.Empty) return -1;

      //The zip file size, in bytes, is stored in the m_ZipFileSize variable.
      //m_ZipFileCount is the number of files in the zip.  This information
      //is useful for some sort of progress information during unzipping.
      if (!GetZipFileSizeAndCount()) return ret;

      DCLIST dclist = new DCLIST();
      dclist.StructVersID = uz_dcl_StructVer;   // Current version of this structure
      dclist.ExtractOnlyNewer = ConvertEnumToInt(m_ExtractOnlyNewer);
      dclist.SpaceToUnderscore = ConvertEnumToInt(m_SpaceToUnderScore);
      dclist.PromptToOverwrite = ConvertEnumToInt(m_PromptToOverWrite);
      dclist.fQuiet = ConvertEnumToInt(m_Quiet);
      dclist.ncflag = ConvertEnumToInt(m_WriteStdOut);
      dclist.ntflag = ConvertEnumToInt (m_TestZip);
      dclist.nvflag = ConvertEnumToInt(m_ExtractOrList);
      dclist.nfflag = ConvertEnumToInt(m_FreshenExisting);
      dclist.nzflag = ConvertEnumToInt(m_DisplayComment);
      dclist.ndflag = ConvertEnumToInt(m_HonorDirectories);
      dclist.noflag = ConvertEnumToInt(m_OverWriteFiles);
      dclist.naflag = ConvertEnumToInt(m_ConvertCR_CRLF);
      dclist.nZIflag = ConvertEnumToInt(m_VerboseZI);
      dclist.B_flag = ConvertEnumToInt(m_UnixFileBackup);
      dclist.C_flag = ConvertEnumToInt(m_CaseSensitivity);
      dclist.D_flag = ConvertEnumToInt(m_RestoreTimeStamps);
      dclist.U_flag = ConvertEnumToInt(m_UTF8NameTranslation);
      dclist.fPrivilege = ConvertEnumToInt(m_Privilege);
      dclist.Zip = m_ZipFileName;
      dclist.ExtractDir = m_ExtractionDirectory;

      USERFUNCTION uf = PrepareUserFunctionCallbackStructure();

      try
      {
        ret = Wiz_SingleEntryUnzip(m_FilesToUnzip.Length, m_FilesToUnzip, m_FilesToExclude.Length,
          m_FilesToExclude, ref dclist, ref uf);
      }
      catch (Exception e)
      {
        MessageBox.Show (e.ToString() + Environment.NewLine +
                         "Last Win32ErrorCode: " + Marshal.GetLastWin32Error());
        //You can check the meaning of return codes here:
        //http://msdn.microsoft.com/library/default.asp?url=/library/en-us/debug/base/system_error_codes__0-499_.asp
      }

      return ret;
    }

    public bool GetZipFileSizeAndCount(ref ulong size, ref ulong fileCount)
    {
      if (!GetZipFileSizeAndCount()) return false;
      size = m_ZipFileSize;
      fileCount = m_ZipFileCount;
      return true;
    }

    public ZipFileEntries GetZipFileContents ()
    {
      int ret = 0;

      DCLIST dclist = new DCLIST();
      dclist.StructVersID = uz_dcl_StructVer;
      dclist.nvflag = ConvertEnumToInt(ExtractOrListEnum.ListContents);
      dclist.Zip = m_ZipFileName;

      USERFUNCTION uf = PrepareUserFunctionCallbackStructure();

      m_ZipFileSize = 0;
      m_ZipFileCount = 0;
      m_ZipFileEntries.Clear();

      //This call will fill the m_ZipFileEntries collection because when the nvflag = ExtractListEnum.ListContents
      //the UZReceiveDLLMessage callback function is called
      try
      {
        ret = Wiz_SingleEntryUnzip(m_FilesToUnzip.Length, m_FilesToUnzip, m_FilesToExclude.Length,
          m_FilesToExclude, ref dclist, ref uf);
      }
      catch(Exception e)
      {
        MessageBox.Show (e.ToString() + Environment.NewLine +
                         "Last Win32ErrorCode: " + Marshal.GetLastWin32Error());
        //You can check the meaning of return codes here:
        //http://msdn.microsoft.com/library/default.asp?url=/library/en-us/debug/base/system_error_codes__0-499_.asp
      }

      return m_ZipFileEntries;
    }

    public string GetZipFileComment()
    {
      //WORK AROUND:
      //This method provides a work around to setting the nzflag of the DCLIST structure = 1, which instructs
      //the dll to extract the zip file comment.  See the KNOWN ISSUES region at the beginning of this code
      //sample.

      //NOTE:
      //Explanation of Big Endian and Little Endian Architecture
      //http://support.microsoft.com/default.aspx?scid=kb;en-us;102025
      //Bytes in the stream are in Big Endian format.  We have to read the bytes and
      //convert to Little Endian format.  That's what the GetLittleEndianByteOrder function
      //does.

      const int CENTRALRECORDENDSIGNATURE = 0x06054b50;
      const int CENTRALRECORDENDSIZE = 22;

      string comment = string.Empty;
      Encoding ae = Encoding.Default;

      if (m_ZipFileName == null | m_ZipFileName == string.Empty) return string.Empty;

      try
      {
        FileStream fs = File.OpenRead(m_ZipFileName);
        long pos = fs.Length - CENTRALRECORDENDSIZE;

        while (GetLittleEndianByteOrder(fs,4) != CENTRALRECORDENDSIGNATURE)
          fs.Seek(pos--, SeekOrigin.Begin);


        int diskNumber = GetLittleEndianByteOrder(fs,2);              /* number of this disk */
        int startingDiskNum = GetLittleEndianByteOrder(fs,2);         /* number of the starting disk */
        int entriesOnrThisDisk = GetLittleEndianByteOrder(fs,2);      /* entries on this disk */
        int totalEntries = GetLittleEndianByteOrder(fs,2);            /* total number of entries */
        int centralDirectoryTotalSize = GetLittleEndianByteOrder(fs,4); /* size of entire central directory */
        int offsetOfCentralDirectory = GetLittleEndianByteOrder(fs,4);  /* offset of central on starting disk */
        //This is what we really want here
        int commentSize = GetLittleEndianByteOrder(fs,2);             /* length of zip file comment */


        byte[] zipFileComment = new byte[commentSize];
        fs.Read(zipFileComment, 0, zipFileComment.Length);

        comment =  ae.GetString(zipFileComment, 0, zipFileComment.Length);
        fs.Close();
      }
      catch (Exception e)
      {
        throw new Exception(e.Message);
      }
      return comment;
    }

    public void Stop ()
    {
      //m_Stop gets returned from the UZDLLService callback.
      //A value of 1 means abort processing.
      m_Stop = 1;
    }

    #endregion

    #region Private Functions

    private int ConvertEnumToInt (System.Enum obj)
    {
      return Convert.ToInt32(obj);
    }

    private bool GetZipFileSizeAndCount()
    {
      int ret = 0;

      DCLIST dclist = new DCLIST();
      dclist.StructVersID = uz_dcl_StructVer;   // Current version of this structure
      dclist.nvflag = ConvertEnumToInt(ExtractOrListEnum.ListContents);
      dclist.Zip = m_ZipFileName;
      dclist.ExtractDir = m_ExtractionDirectory;

      USERFUNCTION uf = PrepareUserFunctionCallbackStructure();

      //Reset these variables
      m_ZipFileSize = 0;
      m_ZipFileCount = 0;

      ret = Wiz_SingleEntryUnzip(m_FilesToUnzip.Length, m_FilesToUnzip, m_FilesToExclude.Length,
                                 m_FilesToExclude, ref dclist, ref uf);

      return (ret == 0);
    }


    private USERFUNCTION PrepareUserFunctionCallbackStructure()
    {
      USERFUNCTION uf = new USERFUNCTION();
      uf.UZDLLPrnt = new UZDLLPrintCallback(UZDLLPrint);
      uf.UZDLLSND = 0; //Not supported
      uf.UZDLLREPLACE = new UZDLLReplaceCallback(UZDLLReplace);
      uf.UZDLLPASSWORD = new UZDLLPasswordCallback(UZDLLPassword);
      uf.UZDLLMESSAGE = new UZReceiveDLLMessageCallback(UZReceiveDLLMessage);
      uf.UZDLLSERVICE = new UZDLLServiceCallback(UZDLLService);
      uf.UZDLLMESSAGE_I32 = null; // not used
      uf.UZDLLSERVICE_I32 = null; // not used

      return uf;
    }

    private int GetLittleEndianByteOrder(FileStream fs, int len)
    {
      int result = 0;
      int n = 0;
      int [] byteArr = new int[len];

      //Pull the bytes from the stream
      for(n=0;n<len;n++)
        byteArr[n] = fs.ReadByte();

      //Left shift the bytes to get a resulting number in little endian format
      for(n=0;n<byteArr.Length;n++)
        result += (byteArr[n] << (n*8));

      return result;
    }

    #endregion
  }
}
