#include <sys/stat.h>
#ifdef umask
#undef umask
#endif
mode_t (*foo)(mode_t) = umask;
int main(void) { return 0; }
