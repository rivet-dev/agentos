#include <unistd.h>
#ifdef truncate
#undef truncate
#endif
int (*foo)(const char *, off_t) = truncate;
int main(void) { return 0; }
