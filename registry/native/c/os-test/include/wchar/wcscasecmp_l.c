#include <wchar.h>
#ifdef wcscasecmp_l
#undef wcscasecmp_l
#endif
int (*foo)(const wchar_t *, const wchar_t *, locale_t) = wcscasecmp_l;
int main(void) { return 0; }
