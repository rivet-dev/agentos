#include <sys/utsname.h>
#ifdef uname
#undef uname
#endif
int (*foo)(struct utsname *) = uname;
int main(void) { return 0; }
