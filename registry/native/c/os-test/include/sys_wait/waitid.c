#include <sys/wait.h>
#ifdef waitid
#undef waitid
#endif
int (*foo)(idtype_t, id_t, siginfo_t *, int) = waitid;
int main(void) { return 0; }
