#include <sys/stat.h>
#ifdef mkdirat
#undef mkdirat
#endif
int (*foo)(int, const char *, mode_t) = mkdirat;
int main(void) { return 0; }
