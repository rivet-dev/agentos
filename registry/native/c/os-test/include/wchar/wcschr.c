#include <wchar.h>
#ifdef wcschr
#undef wcschr
#endif
wchar_t *(*foo)(const wchar_t *, wchar_t) = wcschr;
int main(void) { return 0; }
