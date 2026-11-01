$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$source = @'
using System;
using System.IO;
using System.Text;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

public static class FcRenamePathRepro {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool WriteFile(IntPtr file, byte[] bytes, uint count, out uint written, IntPtr overlapped);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool ReadFile(IntPtr file, byte[] bytes, uint count, out uint read, IntPtr overlapped);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool FlushFileBuffers(IntPtr file);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool SetFilePointerEx(IntPtr file, long offset, out long position, uint origin);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool SetFileInformationByHandle(IntPtr file, int kind, IntPtr data, uint length);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool DuplicateHandle(IntPtr process, IntPtr source, IntPtr targetProcess, out IntPtr target, uint access, bool inherit, uint options);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")]
    static extern IntPtr GetCurrentProcess();
    [StructLayout(LayoutKind.Sequential)]
    struct FileInfo {
        public uint Attributes, CreationLow, CreationHigh, AccessLow, AccessHigh, WriteLow, WriteHigh;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool GetFileInformationByHandle(IntPtr file, out FileInfo info);

    static readonly IntPtr Invalid = new IntPtr(-1);
    static Dictionary<string,object> Row() { return new Dictionary<string,object>(); }
    static bool Valid(IntPtr h) { return h != IntPtr.Zero && h != Invalid; }
    static int Hresult(int code) { return code == 0 ? 0 : unchecked((int)(0x80070000u | (uint)code)); }
    static void Observe(List<object> stages, string name, bool success, int error) {
        var s=Row();s["stage"]=name;s["success"]=success;s["getLastError"]=error;s["hresultFromWin32"]=Hresult(error);stages.Add(s);
    }
    static void Require(List<object> stages, string name, bool success, int error) {
        Observe(stages,name,success,error);if(!success)throw new Win32Exception(error);
    }
    static string Hash(byte[] bytes) {
        using(var h=SHA256.Create())return BitConverter.ToString(h.ComputeHash(bytes)).Replace("-","").ToLowerInvariant();
    }
    static string ReadHash(IntPtr file, List<object> stages, string label) {
        long position;bool ok=SetFilePointerEx(file,0,out position,0);int error=ok?0:Marshal.GetLastWin32Error();Require(stages,label+".seek",ok,error);
        byte[] data=new byte[65536];int total=0;
        while(total<data.Length) {
            byte[] chunk=new byte[Math.Min(8192,data.Length-total)];uint count;
            ok=ReadFile(file,chunk,(uint)chunk.Length,out count,IntPtr.Zero);error=ok?0:Marshal.GetLastWin32Error();Require(stages,label+".read",ok,error);
            if(count==0)throw new InvalidDataException("fixture truncated");Buffer.BlockCopy(chunk,0,data,total,(int)count);total+=(int)count;
        }
        return Hash(data);
    }
    static Dictionary<string,object> RunCase(string root,string label,string target,bool verbatim) {
        var row=Row();var stages=new List<object>();row["case"]=label;row["stages"]=stages;
        row["logicalTargetUtf16Units"]=target.Length;string name=verbatim?@"\\?\"+target:target;
        row["renameNameUtf16Units"]=name.Length;row["verbatim"]=verbatim;
        int offset=IntPtr.Size==4?12:20;row["fileNameOffset"]=offset;row["pointerBytes"]=IntPtr.Size;
        string sourcePath=Path.Combine(root,"source-"+label+".part");
        IntPtr original=IntPtr.Zero,reduced=IntPtr.Zero,buffer=IntPtr.Zero;bool cleaned=false;
        try {
            byte[] fixture=new byte[65536];for(int i=0;i<fixture.Length;i++)fixture[i]=(byte)('A'+i%26);row["expectedFixtureSha256"]=Hash(fixture);
            // Same CREATE_NEW, share0, read/write/DELETE and original write-through/reparse flags as the Rust owned source.
            original=CreateFileW(sourcePath,0xc0010000u,0,IntPtr.Zero,1,0x80200080u,IntPtr.Zero);
            int error=Valid(original)?0:Marshal.GetLastWin32Error();Require(stages,"CreateFile.readWriteDelete.share0",Valid(original),error);
            uint written;bool ok=WriteFile(original,fixture,(uint)fixture.Length,out written,IntPtr.Zero);error=ok?0:Marshal.GetLastWin32Error();Require(stages,"WriteFile.publicFixture",ok,error);
            if(written!=fixture.Length)throw new InvalidDataException("fixture short write");
            ok=FlushFileBuffers(original);error=ok?0:Marshal.GetLastWin32Error();Require(stages,"FlushFileBuffers",ok,error);
            FileInfo before;ok=GetFileInformationByHandle(original,out before);error=ok?0:Marshal.GetLastWin32Error();Require(stages,"OriginalFileIdentity",ok,error);
            // FILE_GENERIC_READ | DELETE, no inheritance/options: actual NewFile::seal reduction.
            ok=DuplicateHandle(GetCurrentProcess(),original,GetCurrentProcess(),out reduced,0x00130089u,false,0);
            error=ok?0:Marshal.GetLastWin32Error();Require(stages,"DuplicateHandle.fileGenericReadDelete",ok,error);
            ok=CloseHandle(original);error=ok?0:Marshal.GetLastWin32Error();Require(stages,"CloseOriginalWriteHandle",ok,error);original=IntPtr.Zero;
            row["sealedFixtureSha256"]=ReadHash(reduced,stages,"SealedHash");
            byte[] tail=Encoding.Unicode.GetBytes(name+"\0");int bytes=checked(offset+tail.Length);buffer=Marshal.AllocHGlobal(bytes);
            for(int i=0;i<bytes;i++)Marshal.WriteByte(buffer,i,0);
            Marshal.WriteIntPtr(buffer,IntPtr.Size==4?4:8,IntPtr.Zero);
            Marshal.WriteInt32(buffer,IntPtr.Size==4?8:16,tail.Length-2);
            Marshal.Copy(tail,0,IntPtr.Add(buffer,offset),tail.Length);
            row["renameInformationBytes"]=bytes;row["fileNameLengthBytesExcludingNul"]=tail.Length-2;
            // FileRenameInfo(3), no overwrite, RootDirectoryNULL, explicit final NUL.
            ok=SetFileInformationByHandle(reduced,3,buffer,(uint)bytes);error=ok?0:Marshal.GetLastWin32Error();
            Observe(stages,"SetFileInformationByHandle.FileRenameInfo",ok,error);row["renameSuccess"]=ok;row["renameGetLastError"]=error;row["renameHresult"]=Hresult(error);
            FileInfo after;bool info=GetFileInformationByHandle(reduced,out after);int ie=info?0:Marshal.GetLastWin32Error();Require(stages,"AfterRenameAttemptIdentity",info,ie);
            row["sameOriginalInode"]=before.Volume==after.Volume&&before.IndexHigh==after.IndexHigh&&before.IndexLow==after.IndexLow;
            row["afterAttemptFixtureSha256"]=ReadHash(reduced,stages,"AfterAttemptHash");row["observationCompleted"]=true;
        } catch(Exception ex) {
            row["observationCompleted"]=false;row["exceptionType"]=ex.GetType().FullName;row["exceptionHresult"]=ex.HResult;
            var native=ex as Win32Exception;if(native!=null)row["exceptionNativeCode"]=native.NativeErrorCode;
        } finally {
            if(buffer!=IntPtr.Zero)Marshal.FreeHGlobal(buffer);
            IntPtr owned=Valid(reduced)?reduced:original;
            if(Valid(owned)) {
                IntPtr disposition=Marshal.AllocHGlobal(1);
                try {
                    Marshal.WriteByte(disposition,1);
                    // Delete this exact newly created original inode, never a path replacement.
                    bool ok=SetFileInformationByHandle(owned,4,disposition,1);int er=ok?0:Marshal.GetLastWin32Error();Observe(stages,"Cleanup.FileDispositionInfo",ok,er);cleaned=ok;
                } finally { Marshal.FreeHGlobal(disposition); }
            }
            if(Valid(reduced)){bool ok=CloseHandle(reduced);int er=ok?0:Marshal.GetLastWin32Error();Observe(stages,"Cleanup.CloseReduced",ok,er);cleaned=cleaned&&ok;}
            if(Valid(original)){bool ok=CloseHandle(original);int er=ok?0:Marshal.GetLastWin32Error();Observe(stages,"Cleanup.CloseOriginal",ok,er);cleaned=cleaned&&ok;}
            row["exactFileCleanupReturned"]=cleaned;
        }
        return row;
    }
    public static Dictionary<string,object> Run(string root) {
        var output=Row();var directories=new List<string>();var cases=new List<object>();output["cases"]=cases;
        string leaf="material-version-0123456789abcdef.txt";string shortParent=Path.Combine(root,"short");
        string current=Path.Combine(root,"long");int wanted=261-1-leaf.Length;
        if(shortParent.Length+1+leaf.Length>=200||current.Length>=wanted||wanted>=248)throw new InvalidDataException("fixture path bounds");
        try {
            Directory.CreateDirectory(shortParent);directories.Add(shortParent);
            Directory.CreateDirectory(current);directories.Add(current);
            while(current.Length<wanted) {
                int remaining=wanted-current.Length;int count=Math.Min(60,remaining-1);if(remaining-count-1==1)count--;
                if(count<1)throw new InvalidDataException("fixture component bounds");
                current=Path.Combine(current,new string('p',count));Directory.CreateDirectory(current);directories.Add(current);
            }
            string longTarget=Path.Combine(current,leaf);if(longTarget.Length!=261)throw new InvalidDataException("fixture exact261");
            output["longParentUtf16Units"]=current.Length;output["longLogicalTargetUtf16Units"]=longTarget.Length;
            cases.Add(RunCase(root,"short-dos",Path.Combine(shortParent,leaf),false));
            cases.Add(RunCase(root,"long-dos",longTarget,false));
            cases.Add(RunCase(root,"long-verbatim",longTarget,true));
        } finally {
            var cleanup=new List<object>();output["directoryCleanup"]=cleanup;
            for(int i=directories.Count-1;i>=0;i--) {
                var row=Row();row["ordinal"]=i;
                try{Directory.Delete(directories[i],false);row["removed"]=true;}
                catch(Exception ex){row["removed"]=false;row["exceptionType"]=ex.GetType().FullName;row["hresult"]=ex.HResult;}
                cleanup.Add(row);
            }
        }
        return output;
    }
}
'@
$result = [ordered]@{schema=1;status='STARTING';scope='Owned synthetic Win11 FileRenameInfo path comparison only';is64BitProcess=[Environment]::Is64BitProcess;pointerBytes=[IntPtr]::Size;registryOrNetworkMutation='NONE';rootCreated=$false;cleanup='NOT_RUN'}
$root=$null;$owned=$false;$owner='fc-rename-repro/v1:'+([Guid]::NewGuid().ToString('N'))
try {
    Add-Type -TypeDefinition $source -Language CSharp
    $root=Join-Path ([IO.Path]::GetTempPath()) ('FC-Rename-'+[Guid]::NewGuid().ToString('N'))
    $null=New-Item -ItemType Directory -Path $root
    $owned=$true;$result.rootCreated=$true;$result.ownedRoot=$root
    $marker=Join-Path $root '.owner'
    $stream=[IO.File]::Open($marker,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try {$bytes=[Text.Encoding]::ASCII.GetBytes($owner);$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)} finally {$stream.Dispose()}
    $result.native=[FcRenamePathRepro]::Run($root)
    $result.status='OBSERVED_THREE_NATIVE_CASES_NOT_PRODUCT_ACCEPTANCE'
} catch {
    $result.status='FAIL';$result.exceptionType=$_.Exception.GetType().FullName;$result.exceptionHresult=$_.Exception.HResult
} finally {
    if($owned) {
        try {
            if(([IO.File]::GetAttributes($root) -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or [IO.File]::ReadAllText((Join-Path $root '.owner')) -cne $owner){throw 'owned marker mismatch'}
            $entries=@([IO.Directory]::GetFileSystemEntries($root))
            if($entries.Count -ne 1 -or $entries[0] -cne (Join-Path $root '.owner')){throw 'owned residue retained'}
            [IO.File]::Delete((Join-Path $root '.owner'));[IO.Directory]::Delete($root,$false)
            $result.cleanup='PASS';$result.rootAbsent=(-not [IO.Directory]::Exists($root))
        } catch {$result.cleanup='RETAINED';$result.cleanupExceptionType=$_.Exception.GetType().FullName}
    } else {$result.cleanup='NO_OWNED_ROOT_CREATED'}
}
$result | ConvertTo-Json -Depth 14 -Compress
