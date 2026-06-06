#include <wchar.h>
#ifdef wcscoll_l
#undef wcscoll_l
#endif
int (*foo)(const wchar_t *, const wchar_t *, locale_t) = wcscoll_l;
int main(void) { return 0; }
