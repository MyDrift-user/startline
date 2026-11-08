Startline Group Policy templates
================================

Copy Startline.admx to C:\Windows\PolicyDefinitions and en-US\Startline.adml to
C:\Windows\PolicyDefinitions\en-US (or the same places in your domain's Central Store).
The settings appear under Administrative Templates > Startline, for computers and for users.
Where both set the same policy, the computer's value wins.

The values are stored in Software\Policies\Startline. Startline picks up changes within a minute
while it's open. Removing a policy brings back the person's own choice.

These two files are written by the app from its list of policies. To write them again:
    startline.exe --write-policy-templates <folder>
