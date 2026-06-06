#include <sys/stat.h>
#ifdef mkfifoat
#undef mkfifoat
#endif
int (*foo)(int, const char *, mode_t) = mkfifoat;
int main(void) { return 0; }
