#include <string.h>
#ifdef strcoll_l
#undef strcoll_l
#endif
int (*foo)(const char *, const char *, locale_t) = strcoll_l;
int main(void) { return 0; }
