#include <unistd.h>
#ifdef getsid
#undef getsid
#endif
pid_t (*foo)(pid_t) = getsid;
int main(void) { return 0; }
