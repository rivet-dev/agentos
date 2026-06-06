#include <unistd.h>
#ifdef getpgid
#undef getpgid
#endif
pid_t (*foo)(pid_t) = getpgid;
int main(void) { return 0; }
