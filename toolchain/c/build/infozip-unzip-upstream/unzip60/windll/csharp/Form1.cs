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

using System;
using System.Drawing;
using System.Collections;
using System.ComponentModel;
using System.Windows.Forms;
using System.Data;

namespace CSharpInfoZip_UnZipSample
{
	/// <summary>
	/// Summary description for Form1.
	/// </summary>
	public class Form1 : System.Windows.Forms.Form
	{
		private System.Windows.Forms.Button btnUnzipArchive;
		private System.Windows.Forms.OpenFileDialog openFileDialog1;
		private System.Windows.Forms.Label label1;
		private System.Windows.Forms.TextBox textBox1;
		private System.Windows.Forms.Label lblProgress;
		private System.Windows.Forms.ProgressBar prgBar;

		//Define the Unzip object
		Unzip m_Unzip = new Unzip();
		private ulong m_CurrentSize;
		private System.Windows.Forms.Button btnListZipFiles;
		private CheckBox chkOverwriteAll;


		/// <summary>
		/// Required designer variable.
		/// </summary>
		private System.ComponentModel.Container components = null;

		public Form1()
		{
			//
			// Required for Windows Form Designer support
			//
			InitializeComponent();

			//
			// TODO: Add any constructor code after InitializeComponent call
			//
		}

		/// <summary>
		/// Clean up any resources being used.
		/// </summary>
		protected override void Dispose( bool disposing )
		{
			if( disposing )
			{
				if (components != null)
				{
					components.Dispose();
				}
			}
			base.Dispose( disposing );
		}

		#region Windows Form Designer generated code
		/// <summary>
		/// Required method for Designer support - do not modify
		/// the contents of this method with the code editor.
		/// </summary>
		private void InitializeComponent()
		{
			this.btnUnzipArchive = new System.Windows.Forms.Button();
			this.openFileDialog1 = new System.Windows.Forms.OpenFileDialog();
			this.label1 = new System.Windows.Forms.Label();
			this.textBox1 = new System.Windows.Forms.TextBox();
			this.lblProgress = new System.Windows.Forms.Label();
			this.prgBar = new System.Windows.Forms.ProgressBar();
			this.btnListZipFiles = new System.Windows.Forms.Button();
			this.chkOverwriteAll = new System.Windows.Forms.CheckBox();
			this.SuspendLayout();
			//
			// btnUnzipArchive
			//
			this.btnUnzipArchive.Location = new System.Drawing.Point(8, 24);
			this.btnUnzipArchive.Name = "btnUnzipArchive";
			this.btnUnzipArchive.Size = new System.Drawing.Size(96, 24);
			this.btnUnzipArchive.TabIndex = 0;
			this.btnUnzipArchive.Text = "Unzip archive...";
			this.btnUnzipArchive.Click += new System.EventHandler(this.btnUnzipArchive_Click);
			//
			// label1
			//
			this.label1.Location = new System.Drawing.Point(8, 64);
			this.label1.Name = "label1";
			this.label1.Size = new System.Drawing.Size(184, 16);
			this.label1.TabIndex = 1;
			this.label1.Text = "Unzip DLL Print callback message:";
			//
			// textBox1
			//
			this.textBox1.Location = new System.Drawing.Point(8, 80);
			this.textBox1.Multiline = true;
			this.textBox1.Name = "textBox1";
			this.textBox1.ScrollBars = System.Windows.Forms.ScrollBars.Vertical;
			this.textBox1.Size = new System.Drawing.Size(464, 120);
			this.textBox1.TabIndex = 2;
			//
			// lblProgress
			//
			this.lblProgress.Location = new System.Drawing.Point(8, 208);
			this.lblProgress.Name = "lblProgress";
			this.lblProgress.Size = new System.Drawing.Size(216, 16);
			this.lblProgress.TabIndex = 3;
			//
			// prgBar
			//
			this.prgBar.Location = new System.Drawing.Point(8, 224);
			this.prgBar.Name = "prgBar";
			this.prgBar.Size = new System.Drawing.Size(216, 16);
			this.prgBar.TabIndex = 4;
			//
			// btnListZipFiles
			//
			this.btnListZipFiles.Location = new System.Drawing.Point(120, 24);
			this.btnListZipFiles.Name = "btnListZipFiles";
			this.btnListZipFiles.Size = new System.Drawing.Size(96, 24);
			this.btnListZipFiles.TabIndex = 5;
			this.btnListZipFiles.Text = "List zip files...";
			this.btnListZipFiles.Click += new System.EventHandler(this.btnListZipFiles_Click);
			//
			// chkOverwriteAll
			//
			this.chkOverwriteAll.AutoSize = true;
			this.chkOverwriteAll.Location = new System.Drawing.Point(255, 29);
			this.chkOverwriteAll.Name = "chkOverwriteAll";
			this.chkOverwriteAll.Size = new System.Drawing.Size(177, 17);
			this.chkOverwriteAll.TabIndex = 6;
			this.chkOverwriteAll.Text = "Overwrite all files without prompt";
			this.chkOverwriteAll.UseVisualStyleBackColor = true;
			//
			// Form1
			//
			this.AutoScaleBaseSize = new System.Drawing.Size(5, 13);
			this.ClientSize = new System.Drawing.Size(480, 254);
			this.Controls.Add(this.chkOverwriteAll);
			this.Controls.Add(this.btnListZipFiles);
			this.Controls.Add(this.prgBar);
			this.Controls.Add(this.lblProgress);
			this.Controls.Add(this.textBox1);
			this.Controls.Add(this.label1);
			this.Controls.Add(this.btnUnzipArchive);
			this.FormBorderStyle = System.Windows.Forms.FormBorderStyle.FixedDialog;
			this.MaximizeBox = false;
			this.MinimizeBox = false;
			this.Name = "Form1";
			this.StartPosition = System.Windows.Forms.FormStartPosition.CenterScreen;
			this.Text = "Form1";
			this.ResumeLayout(false);
			this.PerformLayout();

		}
		#endregion

		/// <summary>
		/// The main entry point for the application.
		/// </summary>
		[STAThread]
		static void Main()
		{
			Application.Run(new Form1());
		}

		#region Event Handlers

		private void btnUnzipArchive_Click(object sender, System.EventArgs e)
		{
			openFileDialog1.ShowDialog();
			string file = openFileDialog1.FileName;

			if (file == null | file == string.Empty) return;

			//Clear the DLL messages output area
			m_CurrentSize = 0;
			prgBar.Value = prgBar.Minimum;
			lblProgress.Text = "";
			textBox1.Text = "";

			//Instantiate the Unzip object
			m_Unzip = new Unzip();

			//NOTE:
			//There are many unzip options.  This sample just demonstrates basic unzip options.
			//Consult the InfoZip documentation for more option information.

			//Set the Unzip object properties
			m_Unzip.ZipFileName = file;
			m_Unzip.HonorDirectories = HonorDirectoriesEnum.True;
			m_Unzip.ExtractOrList = ExtractOrListEnum.Extract;

			//This option sets the DLL to display only the archive comment and
			//then exit immediately.
			//m_Unzip.DisplayComment = DisplayCommentEnum.True;

			//This option switches the DLL into "verbose ZipInfo" mode. The
			//DLL extracts nothing, but instead prints out a verbose technical list
			//of the content of the Zip archive central directory.
			//This option works now, but gets terribly slow when applied to archives
			//with a large number of entries.
			//m_Unzip.VerboseZI = VerboseZIEnum.True;

			if (chkOverwriteAll.Checked)
			{
				m_Unzip.OverWriteFiles = OverWriteFilesEnum.True;
				m_Unzip.PromptToOverWrite = PromptToOverWriteEnum.NotRequired;
			}
			else
			{
				m_Unzip.OverWriteFiles = OverWriteFilesEnum.False;
				m_Unzip.PromptToOverWrite = PromptToOverWriteEnum.Required;
			}

			//NOTE:
			//Directory where the unzipped files are stored.  Change this as appropriate
			m_Unzip.ExtractDirectory = @"c:\temp\unzip";

			//Wire the event handlers to receive the events from the Unzip class
			m_Unzip.ReceivePrintMessage +=new UnZipDLLPrintMessageEventHandler(unZip_ReceivePrintMessage);
			m_Unzip.ReceiveServiceMessage +=new UnZipDLLServiceMessageEventHandler(unZip_ReceiveServiceMessage);

			//Unzip the files
			int ret = m_Unzip.UnZipFiles();

			//Examine the return code
			MessageBox.Show("Done.  Return Code: " + ret.ToString());
		}

		private void btnListZipFiles_Click(object sender, System.EventArgs e)
		{

			openFileDialog1.ShowDialog();
			string file = openFileDialog1.FileName;

			if (file == null | file == string.Empty) return;

			//Clear the DLL messages output area
			prgBar.Value = prgBar.Minimum;
			lblProgress.Text = "";
			textBox1.Text = "";

			//Instantiate the Unzip object
			m_Unzip = new Unzip();

			//NOTE:
			//There are many unzip options.  This sample just demonstrates basic unzip options.
			//Consult the InfoZip documentation for more option information.

			//Set the Unzip object properties
			m_Unzip.ZipFileName = file;
			m_Unzip.HonorDirectories = HonorDirectoriesEnum.True;
			m_Unzip.ExtractOrList = ExtractOrListEnum.ListContents;

			//Wire the event handlers to receive the events from the Unzip class
			m_Unzip.ReceivePrintMessage +=new UnZipDLLPrintMessageEventHandler(unZip_ReceivePrintMessage);
			m_Unzip.ReceiveServiceMessage +=new UnZipDLLServiceMessageEventHandler(unZip_ReceiveServiceMessage);

			//Unzip the files
			ZipFileEntries zfes = m_Unzip.GetZipFileContents();

			//Show the file contents
			frmShowContents frm = new frmShowContents();
			frm.UnzippedFileCollection = zfes;

			//WORK AROUND:
			frm.Comment = m_Unzip.GetZipFileComment();

			frm.ShowDialog(this);

			//Examine the return code
			MessageBox.Show("Done.");

		}

		private void unZip_ReceivePrintMessage(object sender, UnZipDLLPrintMessageEventArgs e)
		{
			textBox1.Text += e.PrintMessage.Replace("\n", Environment.NewLine);
			Application.DoEvents();
		}

		private int unZip_ReceiveServiceMessage(object sender, UnZipDLLServiceMessageEventArgs e)
		{
			m_CurrentSize += e.SizeOfFileEntry;
			prgBar.Value = Convert.ToInt32(100 * Convert.ToDouble(m_CurrentSize)
                                     / Convert.ToDouble(e.ZipFileSize));
			lblProgress.Text = "Unzipping " + m_CurrentSize.ToString("N0") + " of " +
                         e.ZipFileSize.ToString("N0") + " bytes.";
			Application.DoEvents();

			return 0;
		}

		#endregion



	}
}
