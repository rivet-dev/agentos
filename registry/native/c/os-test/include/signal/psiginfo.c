#include <signal.h>
#ifdef psiginfo
#undef psiginfo
#endif
void (*foo)(const siginfo_t *, const char *) = psiginfo;
int main(void) { return 0; }
