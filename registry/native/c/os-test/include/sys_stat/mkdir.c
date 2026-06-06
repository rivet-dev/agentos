#include <sys/stat.h>
#ifdef mkdir
#undef mkdir
#endif
int (*foo)(const char *, mode_t) = mkdir;
int main(void) { return 0; }
